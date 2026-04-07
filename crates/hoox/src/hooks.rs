use std::{
    path::Path,
    process::Command,
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

use crate::config::{
    self,
    CommandSeverity,
    HookCommand,
    Hoox,
    PatternList,
    Verbosity,
    HOOX_FILE_NAME,
    STAGED_HOOKS,
};

pub fn run(hook: &str, args: &[String], ignore_missing: bool) -> Result<()> {
    let cwd = crate::init::find_repo_root(std::env::current_dir()?)?;
    let hoox_path = cwd.join(HOOX_FILE_NAME);

    let content = std::fs::read_to_string(&hoox_path).context("failed to read .hoox.conf")?;

    check_version(&content)?;

    let hoox: Hoox = hocon::de::from_str(&content).context("failed to parse .hoox.conf")?;

    let default_verbosity = hoox.verbosity.unwrap_or(Verbosity::All);
    let default_severity = hoox.severity.unwrap_or(CommandSeverity::Error);

    let commands = match hoox.hooks.get(hook) {
        | Some(cmds) => cmds,
        | None if ignore_missing => return Ok(()),
        | None => return Err(anyhow::anyhow!("hook '{}' not found in .hoox.conf", hook)),
    };

    let changed_files = get_changed_files(hook, &cwd);

    for cmd in commands {
        if !should_run_for_files(cmd, &changed_files)? {
            continue;
        }

        let program = cmd.program.clone().unwrap_or_else(|| vec!["sh".into(), "-c".into()]);
        if program.is_empty() {
            return Err(anyhow::anyhow!("empty program for hook '{}'", hook));
        }

        let mut exec = Command::new(&program[0]);
        exec.args(&program[1..]);

        let script = match (&cmd.command.inline, &cmd.command.file) {
            | (Some(s), None) => s.clone(),
            | (None, Some(f)) => {
                std::fs::read_to_string(cwd.join(f)).with_context(|| format!("failed to read script file: {}", f))?
            },
            | _ => return Err(anyhow::anyhow!("command must have exactly one of 'inline' or 'file'")),
        };

        exec.arg(&script).arg(&hoox_path).args(args);

        let output = exec.output().with_context(|| format!("failed to execute: {}", program[0]))?;

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
            std::process::exit(output.status.code().unwrap_or(1));
        }
    }

    Ok(())
}

fn check_version(content: &str) -> Result<()> {
    let version: config::WithVersion =
        hocon::de::from_str(content).context("failed to parse version from .hoox.conf")?;

    let file_v: Vec<&str> = version.version.split('.').collect();
    let cli_v: Vec<&str> = env!("CARGO_PKG_VERSION").split('.').collect();

    if file_v.len() < 2 || cli_v.len() < 2 {
        return Err(anyhow::anyhow!("invalid version format"));
    }

    // Dev build (0.0.0) accepts any config
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

/// Get the list of changed files relevant to this hook type using libgit2.
/// - Staged hooks: diff between HEAD tree and index (staged files)
/// - Other hooks: diff between HEAD tree and workdir
fn get_changed_files(hook: &str, repo_root: &Path) -> Vec<String> {
    let Ok(repo) = Repository::open(repo_root) else {
        return vec![];
    };

    if STAGED_HOOKS.contains(&hook) {
        staged_files(&repo)
    } else {
        head_diff_files(&repo)
    }
}

/// Collect paths from staged changes (index vs HEAD tree).
fn staged_files(repo: &Repository) -> Vec<String> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(index) = repo.index() else {
        return vec![];
    };
    let Ok(diff) = repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), None) else {
        return vec![];
    };
    collect_diff_paths(&diff)
}

/// Collect paths from workdir changes (workdir vs HEAD tree).
fn head_diff_files(repo: &Repository) -> Vec<String> {
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let Ok(diff) = repo.diff_tree_to_workdir_with_index(head_tree.as_ref(), None) else {
        return vec![];
    };
    collect_diff_paths(&diff)
}

/// Extract file paths from a diff, filtering to added/modified/copied/renamed.
fn collect_diff_paths(diff: &git2::Diff) -> Vec<String> {
    let mut files = vec![];
    let _ = diff.foreach(
        &mut |delta, _| {
            if matches!(
                delta.status(),
                Delta::Added | Delta::Modified | Delta::Copied | Delta::Renamed
            ) {
                if let Some(path) = delta.new_file().path().and_then(|p| p.to_str()) {
                    files.push(path.to_string());
                }
            }
            true
        },
        None,
        None,
        None,
    );
    files
}

/// Determine whether a command should run based on its file selector.
/// - No `files` set or empty selector: always run.
/// - Set but no changed files: skip.
/// - Glob and/or regex set: run if any changed file matches either (OR).
fn should_run_for_files(cmd: &HookCommand, changed_files: &[String]) -> Result<bool> {
    let Some(ref selector) = cmd.files else {
        return Ok(true);
    };

    if !selector.has_patterns() {
        return Ok(true);
    }

    if changed_files.is_empty() {
        return Ok(false);
    }

    if let Some(ref patterns) = selector.glob {
        if matches_glob(patterns, changed_files)? {
            return Ok(true);
        }
    }

    if let Some(ref patterns) = selector.regex {
        if matches_regex(patterns, changed_files)? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if any changed file matches the given glob patterns.
fn matches_glob(patterns: &PatternList, changed_files: &[String]) -> Result<bool> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns.patterns() {
        builder.add(Glob::new(pat).with_context(|| format!("invalid glob pattern: {}", pat))?);
    }
    let glob_set = builder.build().context("failed to build glob set")?;
    Ok(changed_files.iter().any(|f| glob_set.is_match(f)))
}

/// Check if any changed file matches the given regex patterns.
fn matches_regex(patterns: &PatternList, changed_files: &[String]) -> Result<bool> {
    for pat in patterns.patterns() {
        let re = Regex::new(pat).with_context(|| format!("invalid regex pattern: {}", pat))?;
        if changed_files.iter().any(|f| re.is_match(f)) {
            return Ok(true);
        }
    }
    Ok(false)
}
