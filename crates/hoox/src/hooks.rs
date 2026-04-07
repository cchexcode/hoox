use std::{
    io::Write,
    path::Path,
    process::{
        Command,
        Stdio,
    },
};

use anyhow::{
    Context,
    Result,
};
use git2::{
    Delta,
    Repository,
};
use globset::{
    Glob,
    GlobSetBuilder,
};
use regex::Regex;
use serde::Serialize;

use crate::config::{
    self,
    CommandSeverity,
    FileSelector,
    HookCommand,
    Hoox,
    Verbosity,
    HOOX_FILE_NAME,
    STAGED_HOOKS,
};

/// A changed file with its path and change type.
#[derive(Debug, Clone, Serialize)]
pub struct ChangedFile {
    pub path: String,
    #[serde(rename = "type")]
    pub change_type: ChangeType,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
}

pub fn run(hook: &str, args: &[String], ignore_missing: bool) -> Result<()> {
    let repo_root = crate::init::find_repo_root(std::env::current_dir()?)?;
    let hoox_path = repo_root.join(HOOX_FILE_NAME);

    let content = std::fs::read_to_string(&hoox_path).context("failed to read .hoox.conf")?;
    check_version(&content)?;

    let mut hoox: Hoox = hocon::de::from_str(&content).context("failed to parse .hoox.conf")?;

    // Process includes — merge hooks from included files.
    if let Some(includes) = hoox.include.take() {
        for inc_path in &includes {
            let full_path = repo_root.join(inc_path);
            let inc_content = std::fs::read_to_string(&full_path)
                .with_context(|| format!("failed to read included file: {}", inc_path))?;
            let inc_hoox: Hoox = hocon::de::from_str(&inc_content)
                .with_context(|| format!("failed to parse included file: {}", inc_path))?;
            for (hook_name, commands) in inc_hoox.hooks {
                hoox.hooks.entry(hook_name).or_default().extend(commands);
            }
        }
    }

    let default_verbosity = hoox.verbosity.unwrap_or(Verbosity::All);
    let default_severity = hoox.severity.unwrap_or(CommandSeverity::Error);

    let commands = match hoox.hooks.get(hook) {
        | Some(cmds) => cmds,
        | None if ignore_missing => return Ok(()),
        | None => return Err(anyhow::anyhow!("hook '{}' not found in .hoox.conf", hook)),
    };

    let all_changed = get_changed_files(hook, &repo_root);

    let batches = group_commands(commands);
    for batch in batches {
        match batch {
            | Batch::Sequential(cmd) => {
                let result = execute_command(
                    cmd,
                    &repo_root,
                    &hoox_path,
                    args,
                    &all_changed,
                    &default_verbosity,
                    &default_severity,
                )?;
                if let Some(code) = result {
                    std::process::exit(code);
                }
            },
            | Batch::Parallel(cmds) => {
                let results: Vec<Result<Option<i32>>> = std::thread::scope(|s| {
                    let handles: Vec<_> = cmds
                        .iter()
                        .map(|cmd| {
                            s.spawn(|| {
                                execute_command(
                                    cmd,
                                    &repo_root,
                                    &hoox_path,
                                    args,
                                    &all_changed,
                                    &default_verbosity,
                                    &default_severity,
                                )
                            })
                        })
                        .collect();
                    handles.into_iter().map(|h| h.join().expect("thread panicked")).collect()
                });

                for result in results {
                    if let Some(code) = result? {
                        std::process::exit(code);
                    }
                }
            },
        }
    }

    Ok(())
}

/// Execute a single hook command. Returns `Ok(None)` if the command was
/// skipped or succeeded, `Ok(Some(code))` if it failed with severity=error.
fn execute_command(
    cmd: &HookCommand,
    repo_root: &Path,
    hoox_path: &Path,
    hook_args: &[String],
    all_changed: &[ChangedFile],
    default_verbosity: &Verbosity,
    default_severity: &CommandSeverity,
) -> Result<Option<i32>> {
    // 1. Compute matched files.
    let matched_files = match &cmd.files {
        | Some(selector) if selector.has_patterns() => {
            if all_changed.is_empty() {
                return Ok(None);
            }
            let matched = filter_files(selector, all_changed)?;
            if matched.is_empty() {
                return Ok(None);
            }
            matched
        },
        | _ => all_changed.to_vec(),
    };

    // 2. Resolve command script.
    let program = cmd.program.clone().unwrap_or_else(|| vec!["sh".into(), "-c".into()]);
    if program.is_empty() {
        return Err(anyhow::anyhow!("empty program"));
    }

    let script = match (&cmd.command.inline, &cmd.command.file) {
        | (Some(s), None) => s.clone(),
        | (None, Some(f)) => {
            std::fs::read_to_string(repo_root.join(f)).with_context(|| format!("failed to read script file: {}", f))?
        },
        | _ => return Err(anyhow::anyhow!("command must have exactly one of 'inline' or 'file'")),
    };

    // 3. Build process.
    let mut exec = Command::new(&program[0]);
    exec.args(&program[1..]);
    exec.arg(&script);
    exec.arg(hoox_path);
    exec.args(hook_args);
    exec.stdin(Stdio::piped());

    // 4. cwd — relative to repo root.
    if let Some(cwd) = &cmd.cwd {
        exec.current_dir(repo_root.join(cwd));
    }

    // 5. Environment.
    apply_env(&mut exec, cmd, &matched_files)?;

    // 6. Spawn and pipe changed files as JSON array to stdin.
    let stdin_payload = serde_json::to_string(&matched_files).context("failed to serialize changed files")?;
    let mut child = exec.spawn().with_context(|| format!("failed to execute: {}", program[0]))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_payload.as_bytes());
    }
    let output = child.wait_with_output().with_context(|| format!("failed to wait on: {}", program[0]))?;

    // 7. Handle output.
    let verbosity = cmd.verbosity.clone().unwrap_or(default_verbosity.clone());
    let severity = cmd.severity.clone().unwrap_or(default_severity.clone());

    if matches!(verbosity, Verbosity::All | Verbosity::Stdout) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            print!("{}", stdout);
        }
    }

    if matches!(verbosity, Verbosity::All | Verbosity::Stderr) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprint!("{}", stderr);
        }
    }

    if severity == CommandSeverity::Error && !output.status.success() {
        return Ok(Some(output.status.code().unwrap_or(1)));
    }

    Ok(None)
}

/// Build the environment for a command.
/// - If `env.keep` is set: start clean, only inherit vars matching keep
///   patterns.
/// - Otherwise: inherit full environment (default behavior).
/// - Always layer `env.vars` on top.
/// - Always set `HOOX_CHANGED_FILES`.
fn apply_env(exec: &mut Command, cmd: &HookCommand, matched_files: &[ChangedFile]) -> Result<()> {
    if let Some(ref env_config) = cmd.env {
        if let Some(ref keep_patterns) = env_config.keep {
            exec.env_clear();

            let regexes: Vec<Regex> = keep_patterns
                .iter()
                .map(|p| Regex::new(p).with_context(|| format!("invalid keep regex: {}", p)))
                .collect::<Result<_>>()?;

            for (key, val) in std::env::vars() {
                if regexes.iter().any(|r| r.is_match(&key)) {
                    exec.env(&key, &val);
                }
            }
        }

        if let Some(ref vars) = env_config.vars {
            for (key, val) in vars {
                exec.env(key, val);
            }
        }
    }

    let paths: Vec<&str> = matched_files.iter().map(|f| f.path.as_str()).collect();
    exec.env("HOOX_CHANGED_FILES", paths.join("\n"));
    Ok(())
}

// --- Command batching ---

enum Batch<'a> {
    Sequential(&'a HookCommand),
    Parallel(Vec<&'a HookCommand>),
}

/// Group consecutive `parallel = true` commands into batches.
fn group_commands(commands: &[HookCommand]) -> Vec<Batch<'_>> {
    let mut batches: Vec<Batch<'_>> = vec![];
    let mut parallel_batch: Vec<&HookCommand> = vec![];

    for cmd in commands {
        if cmd.parallel.unwrap_or(false) {
            parallel_batch.push(cmd);
        } else {
            if !parallel_batch.is_empty() {
                batches.push(Batch::Parallel(std::mem::take(&mut parallel_batch)));
            }
            batches.push(Batch::Sequential(cmd));
        }
    }

    if !parallel_batch.is_empty() {
        batches.push(Batch::Parallel(parallel_batch));
    }

    batches
}

// --- Version checking ---

fn check_version(content: &str) -> Result<()> {
    let version: config::WithVersion =
        hocon::de::from_str(content).context("failed to parse version from .hoox.conf")?;

    let file_v: Vec<&str> = version.version.split('.').collect();
    let cli_v: Vec<&str> = env!("CARGO_PKG_VERSION").split('.').collect();

    if file_v.len() < 2 || cli_v.len() < 2 {
        return Err(anyhow::anyhow!("invalid version format"));
    }

    if cli_v == ["0", "0", "0"] {
        return Ok(());
    }

    if file_v[0] == "0" && cli_v[0] == "0" {
        if file_v[1] != cli_v[1] {
            return Err(anyhow::anyhow!(
                "incompatible minor version: config {} vs cli {} (must match below 1.0.0)",
                version.version,
                env!("CARGO_PKG_VERSION"),
            ));
        }
    } else if file_v[0] != cli_v[0] {
        return Err(anyhow::anyhow!(
            "incompatible major version: config {} vs cli {}",
            version.version,
            env!("CARGO_PKG_VERSION"),
        ));
    }

    Ok(())
}

// --- Changed file detection (libgit2) ---

fn get_changed_files(hook: &str, repo_root: &Path) -> Vec<ChangedFile> {
    let Ok(repo) = Repository::open(repo_root) else {
        return vec![];
    };

    if STAGED_HOOKS.contains(&hook) {
        staged_files(&repo)
    } else {
        head_diff_files(&repo)
    }
}

fn staged_files(repo: &Repository) -> Vec<ChangedFile> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(index) = repo.index() else {
        return vec![];
    };
    let Ok(diff) = repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), None) else {
        return vec![];
    };
    collect_diff_entries(&diff)
}

fn head_diff_files(repo: &Repository) -> Vec<ChangedFile> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(diff) = repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), None) else {
        return vec![];
    };
    collect_diff_entries(&diff)
}

fn collect_diff_entries(diff: &git2::Diff) -> Vec<ChangedFile> {
    let mut files = vec![];
    let _ = diff.foreach(
        &mut |delta, _| {
            let change_type = match delta.status() {
                | Delta::Added => ChangeType::Added,
                | Delta::Modified => ChangeType::Modified,
                | Delta::Deleted => ChangeType::Deleted,
                | Delta::Renamed => ChangeType::Renamed,
                | Delta::Copied => ChangeType::Copied,
                | _ => return true,
            };
            let path = match delta.status() {
                | Delta::Deleted => delta.old_file().path(),
                | _ => delta.new_file().path(),
            };
            if let Some(p) = path.and_then(|p| p.to_str()) {
                files.push(ChangedFile {
                    path: p.to_string(),
                    change_type,
                });
            }
            true
        },
        None,
        None,
        None,
    );
    files
}

// --- File matching ---

/// Return the subset of changed files matching the selector's patterns.
fn filter_files(selector: &FileSelector, changed_files: &[ChangedFile]) -> Result<Vec<ChangedFile>> {
    let mut matched: Vec<ChangedFile> = Vec::new();

    if let Some(ref patterns) = selector.glob {
        let mut builder = GlobSetBuilder::new();
        for pat in patterns.patterns() {
            builder.add(Glob::new(pat).with_context(|| format!("invalid glob pattern: {}", pat))?);
        }
        let glob_set = builder.build().context("failed to build glob set")?;
        for f in changed_files {
            if glob_set.is_match(&f.path) && !matched.iter().any(|m| m.path == f.path) {
                matched.push(f.clone());
            }
        }
    }

    if let Some(ref patterns) = selector.regex {
        let regexes: Vec<Regex> = patterns
            .patterns()
            .iter()
            .map(|p| Regex::new(p).with_context(|| format!("invalid regex pattern: {}", p)))
            .collect::<Result<_>>()?;
        for f in changed_files {
            if regexes.iter().any(|r| r.is_match(&f.path)) && !matched.iter().any(|m| m.path == f.path) {
                matched.push(f.clone());
            }
        }
    }

    Ok(matched)
}
