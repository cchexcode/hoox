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

pub const HOOX_FILE_NAME: &str = ".hoox.yaml";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithVersion {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Hoox {
    pub version: String,
    pub verbosity: Option<Verbosity>,
    pub severity: Option<CommandSeverity>,
    pub hooks: HashMap<String, Vec<HookCommand>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HookCommand {
    pub command: CommandContent,
    pub program: Option<Vec<String>>,
    pub severity: Option<CommandSeverity>,
    pub verbosity: Option<Verbosity>,
    /// File selector to match against changed files.
    /// Uses `!glob` for glob patterns or `!regex` for regex patterns.
    /// When set, the command only runs if at least one changed file matches.
    pub files: Option<FileSelector>,
}

/// File selector: either glob or regex matching.
/// In YAML, use `!glob` or `!regex` tags (same pattern as `!inline` / `!file`).
///
/// ```yaml
/// files: !glob "**/*.rs"
/// files: !glob ["**/*.rs", "**/*.toml"]
/// files: !regex "src/.*\\.rs$"
/// files: !regex [".*\\.rs$", ".*test.*"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSelector {
    Glob(PatternList),
    Regex(PatternList),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandContent {
    Inline(String),
    File(String),
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
        let yaml = r#"
version: "0.0.0"
hooks:
  "pre-commit":
    - command: !inline "cargo test"
      files: !glob "**/*.rs"
    - command: !inline "npm test"
      files: !glob
        - "**/*.js"
        - "**/*.ts"
    - command: !inline "echo always"
"#;
        let hoox: Hoox = serde_yaml::from_str(yaml).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 3);

        match cmds[0].files.as_ref().unwrap() {
            | FileSelector::Glob(p) => assert_eq!(p.patterns(), vec!["**/*.rs"]),
            | _ => panic!("expected glob"),
        }
        match cmds[1].files.as_ref().unwrap() {
            | FileSelector::Glob(p) => assert_eq!(p.patterns(), vec!["**/*.js", "**/*.ts"]),
            | _ => panic!("expected glob"),
        }
        assert!(cmds[2].files.is_none());
    }

    #[test]
    fn test_deserialize_regex() {
        let yaml = r#"
version: "0.0.0"
hooks:
  "pre-commit":
    - command: !inline "lint"
      files: !regex "src/.*\\.rs$"
    - command: !inline "check"
      files: !regex
        - ".*\\.rs$"
        - ".*test.*"
"#;
        let hoox: Hoox = serde_yaml::from_str(yaml).unwrap();
        let cmds = &hoox.hooks["pre-commit"];
        assert_eq!(cmds.len(), 2);

        match cmds[0].files.as_ref().unwrap() {
            | FileSelector::Regex(p) => assert_eq!(p.patterns(), vec!["src/.*\\.rs$"]),
            | _ => panic!("expected regex"),
        }
        match cmds[1].files.as_ref().unwrap() {
            | FileSelector::Regex(p) => assert_eq!(p.patterns(), vec![".*\\.rs$", ".*test.*"]),
            | _ => panic!("expected regex"),
        }
    }
}
