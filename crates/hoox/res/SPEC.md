HOOX FORMAT SPECIFICATION
=========================

hoox manages Git hooks through a declarative configuration file
named `.hoox.conf` at the repository root. The file uses HOCON
format (Human-Optimized Config Object Notation), a superset of
JSON with comments, substitutions, and multi-line strings.

Run `hoox init` to generate a starter config.
Run `hoox spec` to display this reference.

TABLE OF CONTENTS
-----------------

  1.  CLI Commands
  2.  HOCON Primer
  3.  Top-Level Fields
  4.  Hook Commands
  5.  Command Types (inline / file)
  6.  File Matching (glob / regex)
  7.  Working Directory (cwd)
  8.  Parallel Execution
  9.  Stdin: Changed Files
  10. Environment Variables (env)
  11. HOOX_CHANGED_FILES
  12. Includes (Monorepo)
  13. Version Compatibility
  14. Hook Wrapper Scripts
  15. Available Git Hooks
  16. Complete Example


1. CLI COMMANDS
---------------

  hoox init [-t rust]
    Initialize repository hooks. Creates .hoox.conf from a
    template and installs wrapper scripts in .git/hooks/.

  hoox run <hook> [args...] [--ignore-missing]
    Execute a specific hook. This is what the wrapper scripts
    call. Pass --ignore-missing to silently skip hooks not
    defined in .hoox.conf.

  hoox spec
    Print this format specification to stdout.

  hoox man -o <dir> -f <manpages|markdown>
    Generate manual pages or markdown documentation.

  hoox autocomplete -o <dir> -s <bash|zsh|fish|elvish|powershell>
    Generate shell completion scripts.


2. HOCON PRIMER
---------------

HOCON is a superset of JSON. Key differences:

  - Comments:       // line comment   or   # line comment
  - Unquoted keys:  version = "0.0.0"
  - Equals or colon: key = value   or   key: value
  - Omit commas:    array/object elements separated by newlines
  - Multi-line:     triple quotes """...""" (preserves content
                    exactly; no indent stripping)
  - Substitutions:  ${path.to.value} resolved before deserialization
  - Dotted keys:    command.inline = "x"  is the same as
                    command { inline = "x" }
  - Concatenation:  adjacent strings/values are concatenated

Substitutions are resolved BEFORE serde deserialization, so any
key can reference any other key. Prefix shared definitions with
underscore (e.g. `_shared`) — unknown top-level keys are silently
ignored during deserialization.

Reference: https://github.com/lightbend/config/blob/main/HOCON.md


3. TOP-LEVEL FIELDS
--------------------

  version    (string, REQUIRED)
             Semantic version for config compatibility.
             See section 13.

  verbosity  (string, optional, default: "all")
             Default output behavior for all commands.
             Values: all | none | stdout | stderr

  severity   (string, optional, default: "error")
             Default error handling for all commands.
             Values: error | warn
             "error" = exit on failure; "warn" = continue.

  hooks      (object, REQUIRED)
             Map of hook names to arrays of commands.
             Keys are Git hook names (see section 15).
             May be empty: hooks { }

  include    (array of strings, optional)
             Paths to additional .hoox.conf files.
             See section 12.

Minimal valid config:

  version = "0.0.0"
  hooks { }

Typical config:

  version = "0.0.0"
  verbosity = all
  severity = error
  hooks {
    pre-commit = [ ... ]
    pre-push = [ ... ]
  }


4. HOOK COMMANDS
----------------

Each hook maps to an array of command objects:

  hooks {
    pre-commit = [
      { command.inline = "echo hello" }
      { command.inline = "echo world" }
    ]
  }

Command object fields:

  FIELD        TYPE              DEFAULT     SECTION
  -------      ----              -------     -------
  command      object            REQUIRED    5
  program      array of string   ["sh","-c"] 5
  verbosity    string            (inherit)   3
  severity     string            (inherit)   3
  files        object            (none)      6
  cwd          string            (none)      7
  parallel     bool              false       8
  env          object            (none)      10

Commands execute top-to-bottom unless `parallel` is set.
If a command fails and severity is "error", hoox exits
immediately with the command's exit code.

Every command receives matched changed files as a JSON array
on stdin (see section 9).


5. COMMAND TYPES (inline / file)
---------------------------------

The `command` field is an object with exactly one of two keys:

  command.inline = "shell command string"
  command.file   = "./path/to/script.sh"

Setting BOTH is an error. Setting NEITHER is an error.

INLINE:

  The string is passed as an argument to the program executor.
  With the default program ["sh", "-c"], this runs as a shell
  command.

  Multi-line scripts use HOCON triple quotes. Note: triple
  quotes preserve content exactly, including leading whitespace.
  Do NOT indent the script body:

    command.inline = """set -e
  cargo test
  cargo build"""

  NOT this (would add leading spaces to every line):

    command.inline = """set -e
      cargo test
      cargo build"""

FILE:

  Path relative to repo root. The file's contents are read
  and passed as an argument to the program executor.

  command.file = "./scripts/lint.sh"

PROGRAM:

  Optional custom executor. Default: ["sh", "-c"].

  program = ["python3", "-c"]
  program = ["bash", "-c"]
  program = ["node", "-e"]

  The script (from inline or file) is passed as the FIRST
  argument to the program. Then the hoox config path, then
  any hook arguments from Git.

  Argument order:
    program[0] program[1..] <script> <hoox_path> <git_args>

  In shell terms ($0 = hoox_path, $1+ = git args):
    sh -c '<script>' /abs/path/.hoox.conf <git_args>


6. FILE MATCHING (glob / regex)
--------------------------------

The `files` field controls whether a command runs based on
changed files. It has two optional sub-fields:

  files.glob    Glob patterns (shell-style wildcards)
  files.regex   Regular expression patterns

Each accepts a single string or an array of strings:

  files.glob  = "**/*.rs"
  files.glob  = ["**/*.rs", "**/*.toml"]
  files.regex = "src/.*\\.rs$"
  files.regex = [".*\\.rs$", ".*test.*"]

Both can be set simultaneously — the command runs if a file
matches EITHER (OR logic):

  files { glob = "**/*.rs", regex = ".*migrations.*" }

CHANGED FILE DETECTION:

  Uses libgit2 (no shell-out to git). Only files with status
  Added, Modified, Copied, or Renamed are considered.

  For staged hooks (pre-commit, prepare-commit-msg, commit-msg):
    Index vs HEAD tree (staged files only).

  For all other hooks:
    Workdir vs HEAD (staged + unstaged changes).

BEHAVIOR:

  - No `files` field:       command ALWAYS runs.
  - `files` set, no match:  command is SKIPPED.
  - `files` set, match:     command RUNS.

  The set of matched files is piped to stdin as JSON (section 9)
  and set in HOOX_CHANGED_FILES (section 11).

GLOB SYNTAX (globset crate):

  *        any sequence of characters except /
  **       any sequence of characters including /
  ?        any single character
  [abc]    character class
  [!abc]   negated character class
  {a,b}    alternation

REGEX SYNTAX (Rust regex crate):

  Standard regex. No lookaround. Patterns are matched against
  the full relative path from repo root.
  Example path: "crates/api/src/main.rs"


7. WORKING DIRECTORY (cwd)
--------------------------

Run a command in a specific directory relative to the repo root:

  {
    command.inline = "cargo test"
    cwd = "crates/api"
    files.glob = "crates/api/**/*.rs"
  }

Without `cwd`, commands inherit the current working directory
(wherever git invoked the hook, typically the repo root).

Essential for monorepos where each package has its own tooling.


8. PARALLEL EXECUTION
---------------------

Consecutive commands with `parallel = true` are grouped into
a batch and run concurrently (via std::thread::scope).

Commands without `parallel` (or `parallel = false`) are
sequential barriers that run alone, in order.

  hooks {
    pre-commit = [
      { command.inline = "echo first" }          // sequential
      {
        command.inline = "cargo test"
        cwd = "crates/api"
        parallel = true                          // batch start
      }
      {
        command.inline = "npm test"
        cwd = "packages/web"
        parallel = true                          // same batch
      }
      { command.inline = "echo done" }           // sequential
    ]
  }

Execution:
  1. "echo first" runs alone
  2. "cargo test" and "npm test" run concurrently
  3. After BOTH complete, "echo done" runs

Each command in a parallel batch independently evaluates its
own file filter, env, cwd, and stdin payload.

If any command fails with severity=error, hoox exits after the
entire batch completes (does not kill sibling threads early).


9. STDIN: CHANGED FILES
------------------------

Every command receives its matched changed files as a JSON
array piped to stdin. This is always provided — no opt-in
flag is needed.

  - With a `files` filter: only the files that MATCHED.
  - Without a `files` filter: ALL changed files for this hook.

Each entry is an object with `path` and `type`:

  [
    {"path":"crates/api/src/main.rs","type":"modified"},
    {"path":"crates/api/src/new.rs","type":"added"},
    {"path":"old_file.rs","type":"deleted"}
  ]

CHANGE TYPES (git2::Delta variants, lowercased):

  added      File was added (new file)
  modified   File was modified
  deleted    File was deleted
  renamed    File was renamed
  copied     File was copied

READING IN SHELL (jq):

  Extract just the paths:
  command.inline = "cat | jq -r '.[].path' | xargs prettier --check"

  Filter by type:
  command.inline = """cat | jq -r '.[] | select(.type != "deleted") | .path' \
    | xargs eslint"""

  Full processing:
  command.inline = """CHANGED=$(cat)
  echo "$CHANGED" | jq -r '.[] | "\(.type)\t\(.path)"' | while IFS=$'\t' read -r type path; do
    echo "$type: $path"
  done"""

READING IN PYTHON:

  command.inline = """import json, sys
  for f in json.load(sys.stdin):
      print(f"{f['type']}: {f['path']}")"""
  program = ["python3", "-c"]

READING IN NODE:

  command.inline = """
  const files = JSON.parse(require('fs').readFileSync(0,'utf8'));
  files.filter(f => f.type !== 'del').forEach(f => console.log(f.path));"""
  program = ["node", "-e"]

NOTE: If stdin is not read by the command, it is harmlessly
ignored. Existing commands that don't need the file list work
without any changes.


10. ENVIRONMENT VARIABLES (env)
-------------------------------

The `env` field configures the command's process environment:

  env {
    keep = ["PATH", "HOME", "RUST_.*", "CARGO_.*"]
    vars { RUST_LOG = "debug", CI = "true" }
  }

SUB-FIELDS:

  keep   (array of strings, optional)
         Regex patterns matched against env var NAMES.

         When `keep` IS set:
           1. Environment is CLEARED
           2. Only vars whose names match at least one
              keep pattern are inherited
           3. vars are applied on top
           4. HOOX_CHANGED_FILES is set

         When `keep` IS NOT set:
           1. Full parent environment is inherited (default)
           2. vars are applied on top
           3. HOOX_CHANGED_FILES is set

         The keep patterns use Rust regex syntax and match
         against the full variable name. Use "^PATH$" for
         exact match or "PATH" for substring match.

  vars   (object, optional)
         Key-value pairs of env vars to set. Always applied
         on top, whether or not `keep` is used. Overwrites
         inherited vars with the same name.

EXAMPLES:

  Sandboxed Node.js:

    {
      command.inline = "npm test"
      cwd = "packages/web"
      env {
        keep = ["PATH", "HOME", "USER", "NODE_.*", "NPM_.*"]
        vars { NODE_ENV = "test", CI = "true" }
      }
    }

  Just add extra vars (inherit everything):

    {
      command.inline = "cargo test"
      env.vars { RUST_LOG = "debug" }
    }

  Minimal env (only PATH):

    {
      command.inline = "./scripts/isolated.sh"
      env.keep = ["^PATH$"]
    }


11. HOOX_CHANGED_FILES
-----------------------

Every command receives the HOOX_CHANGED_FILES environment
variable. It contains a newline-separated list of file paths
relative to the repo root.

  - With a `files` filter: only the files that MATCHED.
  - Without a `files` filter: ALL changed files for this hook.

This is always set, even without `env` configuration.

Usage in a script:

  command.inline = """echo "$HOOX_CHANGED_FILES" | while IFS= read -r f; do
  echo "Processing: $f"
  done"""

HOOX_CHANGED_FILES is newline-separated (one file per line).
Stdin (section 9) provides the same list as a JSON array.
Use whichever is more convenient for your tooling.


12. INCLUDES (MONOREPO)
-----------------------

The `include` field imports hooks from additional config files:

  version = "0.0.0"
  include = [
    "crates/api/.hoox.conf"
    "crates/web/.hoox.conf"
    "packages/frontend/.hoox.conf"
  ]
  hooks { }

Paths are relative to the repository root.

MERGE BEHAVIOR:

  Each included file is a complete .hoox.conf. Their hooks are
  APPENDED to the root config's hook arrays.

  If root defines pre-commit = [A, B] and an included file
  defines pre-commit = [C], the result is [A, B, C].

  Include order matters: files are processed top-to-bottom,
  and their commands are appended in that order.

LIMITATIONS:

  - Included files' `include` fields are NOT processed
    (no recursive includes).
  - Version is only checked on the root config.
  - Each included file can use its own HOCON substitutions,
    but substitutions do NOT cross file boundaries.

EXAMPLE (per-package config):

  // crates/api/.hoox.conf
  version = "0.0.0"
  _api {
    test = "cargo test -p api"
  }
  hooks {
    pre-commit = [
      {
        command.inline = ${_api.test}
        cwd = "crates/api"
        files.glob = "crates/api/**/*.rs"
      }
    ]
  }


13. VERSION COMPATIBILITY
-------------------------

The `version` field ensures config/CLI compatibility:

  version = "0.0.0"

Rules:
  - CLI 0.0.0 (dev build): accepts ANY config version.
  - CLI < 1.0.0: config MINOR must match CLI minor.
    Example: CLI 0.3.x requires config 0.3.x
  - CLI >= 1.0.0: config MAJOR must match CLI major.
    Example: CLI 2.x.x requires config 2.x.x

Version is checked BEFORE any hooks execute. On mismatch,
hoox exits with an error message.


14. HOOK WRAPPER SCRIPTS
-------------------------

`hoox init` installs wrapper scripts in .git/hooks/ for all
19 supported Git hooks. Each contains:

  #!/bin/sh
  hoox run --ignore-missing "${0##*/}" "$@"

This delegates to hoox, which reads .hoox.conf and runs the
matching commands. --ignore-missing silently skips hooks not
defined in .hoox.conf.

The hoox binary must be installed and in PATH for hooks to
execute. If hoox is not found, Git will report a hook failure.

Auto-install via build.rs: if hoox is a Rust build dependency,
hooks are installed during `cargo build` (skipped in CI).


15. AVAILABLE GIT HOOKS
-----------------------

hoox supports all 19 standard Git hooks:

  HOOK                  WHEN
  ----                  ----
  applypatch-msg        Invoked by git-am
  commit-msg            After user edits commit message
  post-applypatch       After a patch is applied
  post-checkout         After git-checkout / git-switch
  post-commit           After a commit is created
  post-merge            After a merge completes
  post-receive          After a push is received (server-side)
  post-rewrite          After git-rebase / git-commit --amend
  post-update           After refs are updated (server-side)
  pre-applypatch        Before a patch is applied
  pre-auto-gc           Before automatic garbage collection
  pre-commit            Before a commit is created
  pre-push              Before a push is sent
  pre-rebase            Before a rebase starts
  pre-receive           Before a push is accepted (server-side)
  prepare-commit-msg    Prepare default commit message
  push-to-checkout      Handle push to a checked-out branch
  sendemail-validate    Validate git-send-email patches
  update                Per-ref update check (server-side)

STAGED FILE DETECTION applies to:
  pre-commit, prepare-commit-msg, commit-msg

All other hooks use workdir-vs-HEAD diff.


16. COMPLETE EXAMPLE
--------------------

version = "0.0.0"
verbosity = all
severity = error

// Import per-package hook configs
include = ["packages/api/.hoox.conf"]

// Shared definitions — reuse via ${_shared.key}
_shared {
  cargo_check = """set -e
cargo +nightly fmt --all -- --check
cargo test --all"""
}

hooks {
  pre-commit = [
    // Rust: format + test (only when .rs files change)
    {
      command.inline = ${_shared.cargo_check}
      files.glob = "**/*.rs"
    }

    // JS/TS: lint only the changed files via stdin JSON
    {
      command.inline = "cat | jq -r '.[].path' | xargs eslint"
      files.glob = ["**/*.js", "**/*.ts"]
      parallel = true
      env.vars { NODE_ENV = "development" }
    }

    // CSS: lint changed files, in parallel
    {
      command.inline = "cat | jq -r '.[].path' | xargs stylelint"
      files.glob = "**/*.css"
      parallel = true
    }

    // Python: run in package directory
    {
      command.inline = "pytest"
      cwd = "packages/api"
      files.glob = "packages/api/**/*.py"
    }

    // SQL migration check (regex match)
    {
      command.inline = "check-migrations"
      files.regex = "db/migrations/.*\\.sql$"
    }

    // Script file with custom executor
    {
      command.file = "./scripts/validate.py"
      program = ["python3", "-c"]
      verbosity = stderr
      severity = warn
    }

    // Sandboxed environment
    {
      command.inline = "npm test"
      cwd = "packages/frontend"
      files.glob = "packages/frontend/**"
      env {
        keep = ["PATH", "HOME", "NODE_.*"]
        vars { CI = "true" }
      }
    }
  ]

  pre-push = [
    {
      command.inline = ${_shared.cargo_check}
      files.glob = "**/*.rs"
    }
  ]

  prepare-commit-msg = [
    {
      command.inline = """COMMIT_MSG_FILE=$1
echo "feat: " > $COMMIT_MSG_FILE"""
    }
  ]
}
