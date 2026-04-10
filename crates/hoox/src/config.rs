use std::collections::HashMap;

use serde::{
    Deserialize,
    Serialize,
};

pub const GIT_HOOK_NAMES: [&str; 19] = [
    "applypatch-msg",
    "commit-msg",
    "post-applypatch",
    "post-checkout",
    "post-commit",
    "post-merge",
    "post-receive",
    "post-rewrite",
    "post-update",
    "pre-applypatch",
    "pre-auto-gc",
    "pre-commit",
    "pre-push",
    "pre-rebase",
    "pre-receive",
    "prepare-commit-msg",
    "push-to-checkout",
    "sendemail-validate",
    "update",
];

/// Hooks that operate on staged files (git diff --cached).
pub const STAGED_HOOKS: [&str; 3] = ["pre-commit", "prepare-commit-msg", "commit-msg"];

pub const HOOX_FILE_NAME: &str = ".hoox.conf";
pub const CACHE_FILE_NAME: &str = ".hoox.cache";

#[derive(Debug, Clone, Deserialize)]
pub struct WithVersion {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hoox {
    pub version: String,
    pub verbosity: Option<Verbosity>,
    pub severity: Option<CommandSeverity>,
    pub hooks: HashMap<String, Vec<HookCommand>>,
    /// Paths to additional `.hoox.conf` files (relative to repo root).
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookCommand {
    pub command: CommandSpec,
    pub program: Option<Vec<String>>,
    pub severity: Option<CommandSeverity>,
    pub verbosity: Option<Verbosity>,
    /// File selector to match against changed files.
    pub files: Option<FileSelector>,
    /// Working directory for this command (relative to repo root).
    pub cwd: Option<String>,
    /// Run this command in parallel with adjacent `parallel = true` commands.
    pub parallel: Option<bool>,
    /// Environment variable configuration.
    pub env: Option<EnvConfig>,
    /// Timeout in seconds. Kill the command if it exceeds this duration.
    pub timeout: Option<u64>,
    /// Regex pattern matched against the current branch name.
    /// Command only runs if the branch matches.
    pub branch: Option<String>,
    /// Enable caching. When true, skip this command if the matched files
    /// haven't changed since the last successful run.
    pub cache: Option<bool>,
    /// Number of retry attempts on failure before giving up.
    pub retry: Option<u32>,
}

/// Command specification: exactly one of `inline` or `file` must be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub inline: Option<String>,
    pub file: Option<String>,
}

/// File selector for matching changed files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSelector {
    pub glob: Option<PatternList>,
    pub regex: Option<PatternList>,
}

impl FileSelector {
    pub fn has_patterns(&self) -> bool {
        self.glob.is_some() || self.regex.is_some()
    }
}

/// A single pattern or list of patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatternList {
    Single(String),
    Multiple(Vec<String>),
}

impl PatternList {
    pub fn patterns(&self) -> Vec<&str> {
        match self {
            | PatternList::Single(s) => vec![s.as_str()],
            | PatternList::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Environment variable configuration for a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvConfig {
    pub vars: Option<HashMap<String, String>>,
    pub keep: Option<Vec<String>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verbosity {
    All,
    None,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSeverity {
    Error,
    Warn,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_deserialize_glob() {
        let conf = r#"
version = "0.0.0"
hooks {
  pre-commit = [
    { command.inline = "cargo test", files.glob = "**/*.rs" }
    { command.inline = "npm test", files.glob = ["**/*.js", "**/*.ts"] }
    { command.inline = "echo always" }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].files.as_ref().unwrap().glob.as_ref().unwrap().patterns(), vec![
            "**/*.rs"
        ]);
        assert_eq!(cmds[1].files.as_ref().unwrap().glob.as_ref().unwrap().patterns(), vec![
            "**/*.js", "**/*.ts"
        ]);
        assert!(cmds[2].files.is_none());
    }

    #[test]
    fn test_deserialize_regex() {
        let conf = r#"
version = "0.0.0"
hooks {
  pre-commit = [
    { command.inline = "lint", files.regex = "src/.*\\.rs$" }
    { command.inline = "check", files.regex = [".*\\.rs$", ".*test.*"] }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(
            cmds[0].files.as_ref().unwrap().regex.as_ref().unwrap().patterns(),
            vec!["src/.*\\.rs$"]
        );
        assert_eq!(
            cmds[1].files.as_ref().unwrap().regex.as_ref().unwrap().patterns(),
            vec![".*\\.rs$", ".*test.*"]
        );
    }

    #[test]
    fn test_deserialize_all_features() {
        let conf = r#"
version = "0.0.0"
include = ["sub/.hoox.conf"]
hooks {
  pre-commit = [
    {
      command.inline = "cargo test"
      cwd = "crates/api"
      files.glob = "crates/api/**/*.rs"
      parallel = true
      timeout = 120
      branch = "main|develop"
      cache = true
      env {
        keep = ["PATH", "HOME"]
        vars { CI = "true" }
      }
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let c = &hoox.hooks["pre-commit"][0];
        assert_eq!(c.cwd.as_ref().unwrap(), "crates/api");
        assert_eq!(c.parallel, Some(true));
        assert_eq!(c.timeout, Some(120));
        assert_eq!(c.branch.as_ref().unwrap(), "main|develop");
        assert_eq!(c.cache, Some(true));
        assert_eq!(hoox.include.as_ref().unwrap(), &["sub/.hoox.conf"]);
    }
}
