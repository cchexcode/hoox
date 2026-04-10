use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    process::{
        Command,
        Stdio,
    },
    time::{
        Duration,
        Instant,
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
use serde::{
    Deserialize,
    Serialize,
};
use sha2::{
    Digest,
    Sha256,
};

use crate::config::{
    self,
    CommandSeverity,
    FileSelector,
    HookCommand,
    Hoox,
    Verbosity,
    CACHE_FILE_NAME,
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

// --- Cache ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CacheStore {
    /// Map of "hook:command_index" -> sha256 hash of matched file paths +
    /// contents.
    entries: HashMap<String, String>,
}

impl CacheStore {
    fn load(repo_root: &Path) -> Self {
        let path = repo_root.join(CACHE_FILE_NAME);
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self, repo_root: &Path) {
        let path = repo_root.join(CACHE_FILE_NAME);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Hash matched file paths + their contents for cache comparison.
fn compute_cache_hash(repo_root: &Path, matched_files: &[ChangedFile]) -> String {
    let mut hasher = Sha256::new();
    for f in matched_files {
        hasher.update(f.path.as_bytes());
        hasher.update(b"\0");
        if let Ok(content) = std::fs::read(repo_root.join(&f.path)) {
            hasher.update(&content);
        }
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

// --- Run result tracking ---

#[derive(Debug, Clone, Copy)]
enum RunResult {
    Ran,
    Skipped,
    Cached,
    Failed(i32),
}

#[derive(Debug, Default)]
struct RunStats {
    ran: u32,
    skipped: u32,
    cached: u32,
    failed: u32,
}

impl RunStats {
    fn record(&mut self, r: RunResult) {
        match r {
            | RunResult::Ran => self.ran += 1,
            | RunResult::Skipped => self.skipped += 1,
            | RunResult::Cached => self.cached += 1,
            | RunResult::Failed(_) => self.failed += 1,
        }
    }

    fn print_summary(&self) {
        let total = self.ran + self.skipped + self.cached + self.failed;
        if total == 0 {
            return;
        }
        let mut parts = vec![];
        if self.ran > 0 {
            parts.push(format!("{} passed", self.ran));
        }
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        if self.cached > 0 {
            parts.push(format!("{} cached", self.cached));
        }
        eprintln!("hoox: {}", parts.join(", "));
    }
}

// --- Public API ---

pub fn run(hook: &str, args: &[String], ignore_missing: bool, dry_run: bool) -> Result<()> {
    let repo_root = match crate::init::find_repo_root(std::env::current_dir()?) {
        | Ok(r) => r,
        | Err(_) if ignore_missing => return Ok(()),
        | Err(e) => return Err(e),
    };
    let hoox_path = repo_root.join(HOOX_FILE_NAME);

    let content = match std::fs::read_to_string(&hoox_path) {
        | Ok(c) => c,
        | Err(_) if ignore_missing => return Ok(()),
        | Err(e) => return Err(anyhow::Error::new(e).context("failed to read .hoox.conf")),
    };
    check_version(&content)?;

    let hoox = load_with_includes(&content, &repo_root)?;

    let default_verbosity = hoox.verbosity.unwrap_or(Verbosity::All);
    let default_severity = hoox.severity.unwrap_or(CommandSeverity::Error);

    let commands = match hoox.hooks.get(hook) {
        | Some(cmds) => cmds,
        | None if ignore_missing => return Ok(()),
        | None => return Err(anyhow::anyhow!("hook '{}' not found in .hoox.conf", hook)),
    };

    let all_changed = get_changed_files(hook, &repo_root);
    let current_branch = get_current_branch(&repo_root);

    let mut cache = CacheStore::load(&repo_root);
    let mut cache_dirty = false;
    let mut stats = RunStats::default();
    let mut exit_code: Option<i32> = None;

    let batches = group_commands(commands);
    for batch in batches {
        if exit_code.is_some() {
            break;
        }
        match batch {
            | Batch::Sequential(idx, cmd) => {
                let result = run_one(
                    cmd,
                    hook,
                    idx,
                    &repo_root,
                    &hoox_path,
                    args,
                    &all_changed,
                    current_branch.as_deref(),
                    &default_verbosity,
                    &default_severity,
                    &mut cache,
                    &mut cache_dirty,
                    dry_run,
                )?;
                stats.record(result);
                if let RunResult::Failed(code) = result {
                    exit_code = Some(code);
                }
            },
            | Batch::Parallel(cmds) => {
                let cache_snapshot = cache.entries.clone();
                let branch_ref = current_branch.as_deref();
                let repo_ref = &repo_root;
                let hoox_ref = &hoox_path;
                let changed_ref = &all_changed;
                let verb_ref = &default_verbosity;
                let sev_ref = &default_severity;
                let results: Vec<(Result<RunResult>, Option<(String, String)>)> = std::thread::scope(|s| {
                    cmds.iter()
                        .map(|&(idx, cmd)| {
                            let snapshot = cache_snapshot.clone();
                            s.spawn(move || {
                                let mut local_cache = CacheStore {
                                    entries: snapshot.clone(),
                                };
                                let mut dummy_dirty = false;
                                let result = run_one(
                                    cmd,
                                    hook,
                                    idx,
                                    repo_ref,
                                    hoox_ref,
                                    args,
                                    changed_ref,
                                    branch_ref,
                                    verb_ref,
                                    sev_ref,
                                    &mut local_cache,
                                    &mut dummy_dirty,
                                    dry_run,
                                );
                                let cache_key = format!("{}:{}", hook, idx);
                                let cache_update = local_cache
                                    .entries
                                    .get(&cache_key)
                                    .filter(|v| snapshot.get(&cache_key) != Some(v))
                                    .map(|v| (cache_key, v.clone()));
                                (result, cache_update)
                            })
                        })
                        .collect::<Vec<_>>()
                        .into_iter()
                        .map(|h| h.join().expect("thread panicked"))
                        .collect()
                });

                for (result, cache_update) in results {
                    if let Some((key, val)) = cache_update {
                        cache.entries.insert(key, val);
                        cache_dirty = true;
                    }
                    let r = result?;
                    stats.record(r);
                    if let RunResult::Failed(code) = r {
                        if exit_code.is_none() {
                            exit_code = Some(code);
                        }
                    }
                }
            },
        }
    }

    if cache_dirty {
        cache.save(&repo_root);
    }

    if !dry_run {
        stats.print_summary();
    }

    if let Some(code) = exit_code {
        std::process::exit(code);
    }

    Ok(())
}

/// Validate .hoox.conf: parse, check version, verify file refs and patterns.
pub fn validate() -> Result<()> {
    let repo_root = crate::init::find_repo_root(std::env::current_dir()?)?;
    let hoox_path = repo_root.join(HOOX_FILE_NAME);
    let content = std::fs::read_to_string(&hoox_path).context("failed to read .hoox.conf")?;

    check_version(&content)?;
    let hoox = load_with_includes(&content, &repo_root)?;

    let mut errors: Vec<String> = vec![];

    for (hook_name, commands) in &hoox.hooks {
        for (i, cmd) in commands.iter().enumerate() {
            let label = format!("hooks.{}[{}]", hook_name, i);

            // Validate command spec.
            match (&cmd.command.inline, &cmd.command.file) {
                | (Some(_), Some(_)) => errors.push(format!("{}: both inline and file set", label)),
                | (None, None) => errors.push(format!("{}: neither inline nor file set", label)),
                | (_, Some(f)) => {
                    let path = repo_root.join(f);
                    if !path.exists() {
                        errors.push(format!("{}: file not found: {}", label, f));
                    }
                },
                | _ => {},
            }

            // Validate glob patterns.
            if let Some(ref files) = cmd.files {
                if let Some(ref patterns) = files.glob {
                    for pat in patterns.patterns() {
                        if let Err(e) = Glob::new(pat) {
                            errors.push(format!("{}: invalid glob '{}': {}", label, pat, e));
                        }
                    }
                }
                if let Some(ref patterns) = files.regex {
                    for pat in patterns.patterns() {
                        if let Err(e) = Regex::new(pat) {
                            errors.push(format!("{}: invalid regex '{}': {}", label, pat, e));
                        }
                    }
                }
            }

            // Validate branch regex.
            if let Some(ref branch_pat) = cmd.branch {
                if let Err(e) = Regex::new(branch_pat) {
                    errors.push(format!("{}: invalid branch regex '{}': {}", label, branch_pat, e));
                }
            }

            // Validate env.keep regexes.
            if let Some(ref env) = cmd.env {
                if let Some(ref keep) = env.keep {
                    for pat in keep {
                        if let Err(e) = Regex::new(pat) {
                            errors.push(format!("{}: invalid keep regex '{}': {}", label, pat, e));
                        }
                    }
                }
            }

            // Validate program is not empty.
            if let Some(ref prog) = cmd.program {
                if prog.is_empty() {
                    errors.push(format!("{}: program array is empty", label));
                }
            }
        }
    }

    if errors.is_empty() {
        println!("ok");
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  {}", e);
        }
        Err(anyhow::anyhow!("{} validation error(s)", errors.len()))
    }
}

/// List configured hooks and their commands.
pub fn list() -> Result<()> {
    let repo_root = crate::init::find_repo_root(std::env::current_dir()?)?;
    let hoox_path = repo_root.join(HOOX_FILE_NAME);
    let content = std::fs::read_to_string(&hoox_path).context("failed to read .hoox.conf")?;
    let hoox = load_with_includes(&content, &repo_root)?;

    let mut hook_names: Vec<&String> = hoox.hooks.keys().collect();
    hook_names.sort();

    for hook_name in hook_names {
        let commands = &hoox.hooks[hook_name];
        println!(
            "{} ({} command{})",
            hook_name,
            commands.len(),
            if commands.len() == 1 { "" } else { "s" }
        );
        for (i, cmd) in commands.iter().enumerate() {
            let cmd_desc = match (&cmd.command.inline, &cmd.command.file) {
                | (Some(s), _) => {
                    let first_line = s.lines().next().unwrap_or(s);
                    if first_line.len() > 60 {
                        format!("inline: {}...", &first_line[..57])
                    } else {
                        format!("inline: {}", first_line)
                    }
                },
                | (_, Some(f)) => format!("file: {}", f),
                | _ => "invalid".into(),
            };

            let mut tags = vec![];
            if cmd.files.as_ref().map_or(false, |f| f.has_patterns()) {
                tags.push("files");
            }
            if cmd.parallel.unwrap_or(false) {
                tags.push("parallel");
            }
            if cmd.cwd.is_some() {
                tags.push("cwd");
            }
            if cmd.timeout.is_some() {
                tags.push("timeout");
            }
            if cmd.branch.is_some() {
                tags.push("branch");
            }
            if cmd.cache.unwrap_or(false) {
                tags.push("cache");
            }
            if cmd.retry.is_some() {
                tags.push("retry");
            }
            if cmd.env.is_some() {
                tags.push("env");
            }

            let tag_str = if tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", tags.join(", "))
            };

            println!("  {}: {}{}", i, cmd_desc, tag_str);
        }
    }

    Ok(())
}

/// Delete the .hoox.cache file.
pub fn clean() -> Result<()> {
    let repo_root = crate::init::find_repo_root(std::env::current_dir()?)?;
    let cache_path = repo_root.join(CACHE_FILE_NAME);
    if cache_path.exists() {
        std::fs::remove_file(&cache_path).context("failed to delete .hoox.cache")?;
        println!("deleted .hoox.cache");
    } else {
        println!("no .hoox.cache to delete");
    }
    Ok(())
}

// --- Internal helpers ---

fn load_with_includes(content: &str, repo_root: &Path) -> Result<Hoox> {
    let mut hoox: Hoox = hocon::de::from_str(content).context("failed to parse .hoox.conf")?;

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

    Ok(hoox)
}

/// Run a single command with all checks (branch, files, cache, timeout,
/// retry, dry-run).
#[allow(clippy::too_many_arguments)]
fn run_one(
    cmd: &HookCommand,
    hook: &str,
    cmd_index: usize,
    repo_root: &Path,
    hoox_path: &Path,
    hook_args: &[String],
    all_changed: &[ChangedFile],
    current_branch: Option<&str>,
    default_verbosity: &Verbosity,
    default_severity: &CommandSeverity,
    cache: &mut CacheStore,
    cache_dirty: &mut bool,
    dry_run: bool,
) -> Result<RunResult> {
    let cmd_label = format!("{}:{}", hook, cmd_index);

    // 1. Branch filter.
    if let Some(ref branch_pat) = cmd.branch {
        let re = Regex::new(branch_pat).with_context(|| format!("invalid branch regex: {}", branch_pat))?;
        match current_branch {
            | Some(b) if re.is_match(b) => {},
            | _ => {
                if dry_run {
                    println!("[skip] {} (branch mismatch)", cmd_label);
                }
                return Ok(RunResult::Skipped);
            },
        }
    }

    // 2. File matching.
    let matched_files = match &cmd.files {
        | Some(selector) if selector.has_patterns() => {
            if all_changed.is_empty() {
                if dry_run {
                    println!("[skip] {} (no changed files)", cmd_label);
                }
                return Ok(RunResult::Skipped);
            }
            let matched = filter_files(selector, all_changed)?;
            if matched.is_empty() {
                if dry_run {
                    println!("[skip] {} (no files match filter)", cmd_label);
                }
                return Ok(RunResult::Skipped);
            }
            matched
        },
        | _ => all_changed.to_vec(),
    };

    // 3. Cache check (opt-in).
    let cache_key = cmd_label.clone();
    if cmd.cache.unwrap_or(false) {
        let current_hash = compute_cache_hash(repo_root, &matched_files);
        if let Some(cached_hash) = cache.entries.get(&cache_key) {
            if *cached_hash == current_hash {
                if dry_run {
                    println!("[skip] {} (cached)", cache_key);
                }
                return Ok(RunResult::Cached);
            }
        }
    }

    // 4. Resolve command.
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

    // 5. Dry run — print and return.
    if dry_run {
        let file_count = matched_files.len();
        let script_preview = script.lines().next().unwrap_or(&script);
        println!(
            "[ run] {} ({} file{}) {}",
            cache_key,
            file_count,
            if file_count == 1 { "" } else { "s" },
            script_preview,
        );
        return Ok(RunResult::Ran);
    }

    // 6. Execute with retry.
    let max_attempts = cmd.retry.unwrap_or(0) + 1;
    let verbosity = cmd.verbosity.clone().unwrap_or(default_verbosity.clone());
    let severity = cmd.severity.clone().unwrap_or(default_severity.clone());
    let stdin_payload = serde_json::to_string(&matched_files).context("failed to serialize changed files")?;

    for attempt in 1..=max_attempts {
        let mut exec = Command::new(&program[0]);
        exec.args(&program[1..]);
        exec.arg(&script);
        exec.arg(hoox_path);
        exec.args(hook_args);
        exec.stdin(Stdio::piped());

        if let Some(ref cwd) = cmd.cwd {
            exec.current_dir(repo_root.join(cwd));
        }

        apply_env(&mut exec, cmd, &matched_files)?;

        let mut child = exec.spawn().with_context(|| format!("failed to execute: {}", program[0]))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(stdin_payload.as_bytes());
        }

        // Wait with optional timeout.
        let maybe_output = if let Some(timeout_secs) = cmd.timeout {
            let deadline = Instant::now() + Duration::from_secs(timeout_secs);
            loop {
                match child.try_wait() {
                    | Ok(Some(_)) => break Some(child.wait_with_output()?),
                    | Ok(None) => {
                        if Instant::now() >= deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            if attempt < max_attempts {
                                eprintln!("hoox: {}: timed out (attempt {}/{})", cache_key, attempt, max_attempts);
                                break None; // retry
                            }
                            return Err(anyhow::anyhow!("{}: timed out after {}s", cache_key, timeout_secs));
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    },
                    | Err(e) => return Err(e).context("failed to wait on process"),
                }
            }
        } else {
            Some(child.wait_with_output().with_context(|| format!("failed to wait on: {}", program[0]))?)
        };

        let Some(output) = maybe_output else {
            continue; // timed out, retry
        };

        // Handle output.
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

        if output.status.success() {
            // Update cache on success.
            if cmd.cache.unwrap_or(false) {
                let hash = compute_cache_hash(repo_root, &matched_files);
                cache.entries.insert(cache_key, hash);
                *cache_dirty = true;
            }
            return Ok(RunResult::Ran);
        }

        // Failed — retry or give up.
        if attempt < max_attempts {
            eprintln!(
                "hoox: {}: failed (attempt {}/{}, retrying)",
                cache_key, attempt, max_attempts
            );
        } else if severity == CommandSeverity::Error {
            return Ok(RunResult::Failed(output.status.code().unwrap_or(1)));
        }
    }

    Ok(RunResult::Ran)
}

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
    Sequential(usize, &'a HookCommand),
    Parallel(Vec<(usize, &'a HookCommand)>),
}

fn group_commands(commands: &[HookCommand]) -> Vec<Batch<'_>> {
    let mut batches: Vec<Batch<'_>> = vec![];
    let mut parallel_batch: Vec<(usize, &HookCommand)> = vec![];

    for (i, cmd) in commands.iter().enumerate() {
        if cmd.parallel.unwrap_or(false) {
            parallel_batch.push((i, cmd));
        } else {
            if !parallel_batch.is_empty() {
                batches.push(Batch::Parallel(std::mem::take(&mut parallel_batch)));
            }
            batches.push(Batch::Sequential(i, cmd));
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

// --- Branch detection ---

fn get_current_branch(repo_root: &Path) -> Option<String> {
    let repo = Repository::open(repo_root).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().map(|s| s.to_string())
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
