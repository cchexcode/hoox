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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookCommand {
    pub command: CommandSpec,
    pub program: Option<Vec<String>>,
    pub severity: Option<CommandSeverity>,
    pub verbosity: Option<Verbosity>,
    /// File selector to match against changed files.
    /// Set `glob` for glob patterns, `regex` for regex patterns, or both (OR).
    pub files: Option<FileSelector>,
}

/// Command specification: exactly one of `inline` or `file` must be set.
///
/// ```hocon
/// command.inline = "echo hello"
/// command.file = "./scripts/lint.sh"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    /// Inline shell command string.
    pub inline: Option<String>,
    /// Path to a script file (relative to repo root).
    pub file: Option<String>,
}

/// File selector for matching changed files.
/// Set `glob` for glob patterns, `regex` for regex, or both (OR logic).
///
/// ```hocon
/// files.glob = "**/*.rs"
/// files.glob = ["**/*.rs", "**/*.toml"]
/// files.regex = "src/.*\\.rs$"
/// files { glob = "**/*.rs", regex = ".*test.*" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSelector {
    /// Glob patterns to match against changed file paths.
    pub glob: Option<PatternList>,
    /// Regex patterns to match against changed file paths.
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
    {
      command.inline = "cargo test"
      files.glob = "**/*.rs"
    }
    {
      command.inline = "npm test"
      files.glob = ["**/*.js", "**/*.ts"]
    }
    {
      command.inline = "echo always"
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 3);

        let f0 = cmds[0].files.as_ref().unwrap();
        assert_eq!(f0.glob.as_ref().unwrap().patterns(), vec!["**/*.rs"]);
        assert!(f0.regex.is_none());

        let f1 = cmds[1].files.as_ref().unwrap();
        assert_eq!(f1.glob.as_ref().unwrap().patterns(), vec!["**/*.js", "**/*.ts"]);

        assert!(cmds[2].files.is_none());
    }

    #[test]
    fn test_deserialize_regex() {
        let conf = r#"
version = "0.0.0"
hooks {
  pre-commit = [
    {
      command.inline = "lint"
      files.regex = "src/.*\\.rs$"
    }
    {
      command.inline = "check"
      files.regex = [".*\\.rs$", ".*test.*"]
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 2);

        let f0 = cmds[0].files.as_ref().unwrap();
        assert!(f0.glob.is_none());
        assert_eq!(f0.regex.as_ref().unwrap().patterns(), vec!["src/.*\\.rs$"]);

        let f1 = cmds[1].files.as_ref().unwrap();
        assert_eq!(f1.regex.as_ref().unwrap().patterns(), vec![".*\\.rs$", ".*test.*"]);
    }

    #[test]
    fn test_deserialize_both_glob_and_regex() {
        let conf = r#"
version = "0.0.0"
hooks {
  pre-commit = [
    {
      command.inline = "check"
      files { glob = "**/*.rs", regex = ".*test.*" }
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        let f = cmds[0].files.as_ref().unwrap();
        assert!(f.glob.is_some());
        assert!(f.regex.is_some());
    }

    #[test]
    fn test_deserialize_substitution() {
        let conf = r#"
version = "0.0.0"
_shared {
  cargo = "cargo test --all"
}
hooks {
  pre-commit = [
    {
      command.inline = ${_shared.cargo}
      files.glob = "**/*.rs"
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command.inline.as_ref().unwrap(), "cargo test --all");
    }

    #[test]
    fn test_deserialize_file_command() {
        let conf = r#"
version = "0.0.0"
hooks {
  pre-commit = [
    {
      command.file = "./scripts/lint.sh"
      verbosity = stderr
      severity = warn
    }
  ]
}
"#;
        let hoox: Hoox = hocon::de::from_str(conf).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert!(cmds[0].command.inline.is_none());
        assert_eq!(cmds[0].command.file.as_ref().unwrap(), "./scripts/lint.sh");
        assert_eq!(cmds[0].verbosity, Some(Verbosity::Stderr));
        assert_eq!(cmds[0].severity, Some(CommandSeverity::Warn));
    }
}
