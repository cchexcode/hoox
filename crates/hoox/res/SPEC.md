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
  7.  Branch Filter
  8.  Working Directory (cwd)
  9.  Parallel Execution
  10. Timeout
  11. Retry
  12. Caching
  13. Stdin: Changed Files
  14. Environment Variables (env)
  15. HOOX_CHANGED_FILES
  16. Includes (Monorepo)
  17. Version Compatibility
  18. Hook Wrapper Scripts
  19. Available Git Hooks
  20. Complete Example


1. CLI COMMANDS
---------------

  hoox init [-t rust]
    Initialize repository hooks. Creates .hoox.conf from a
    template and installs wrapper scripts in .git/hooks/.

  hoox run <hook> [args...] [--ignore-missing] [--dry-run]
    Execute a specific hook. This is what the wrapper scripts
    call. Pass --ignore-missing to silently skip hooks not
    defined in .hoox.conf.
    Pass --dry-run to show which commands would run (and which
    would be skipped) without executing anything.
    After execution, prints a status summary to stderr:
      hoox: 3 passed, 1 failed, 2 skipped, 1 cached

  hoox validate
    Parse .hoox.conf, check version compatibility, verify that
    command.file paths exist, and validate all glob, regex, and
    branch patterns. Prints "ok" or lists errors.

  hoox list
    Print all configured hooks and their commands in a summary
    format. Shows command index, first line of script, and tags
    for active features (files, parallel, cwd, timeout, branch,
    cache, retry, env).

  hoox clean
    Delete the .hoox.cache file.

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
             See section 16.

  verbosity  (string, optional, default: "all")
             Default output behavior for all commands.
             Values: all | none | stdout | stderr

  severity   (string, optional, default: "error")
             Default error handling for all commands.
             Values: error | warn
             "error" = exit on failure; "warn" = continue.

  hooks      (object, REQUIRED)
             Map of hook names to arrays of commands.
             Keys are Git hook names (see section 18).
             May be empty: hooks { }

  include    (array of strings, optional)
             Paths to additional .hoox.conf files.
             See section 15.

Minimal valid config:

  version = "0.0.0"
  hooks { }


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
  branch       string            (none)      7
  cwd          string            (none)      8
  parallel     bool              false       9
  timeout      integer           (none)      10
  retry        integer           0           11
  cache        bool              false       12
  env          object            (none)      14

Commands execute top-to-bottom unless `parallel` is set.
If a command fails and severity is "error", hoox exits
immediately with the command's exit code.

Every command receives matched changed files as a JSON array
on stdin (see section 12).


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

FILE:

  Path relative to repo root. The file's contents are read
  and passed as an argument to the program executor.

  command.file = "./scripts/lint.sh"

PROGRAM:

  Optional custom executor. Default: ["sh", "-c"].

  program = ["python3", "-c"]
  program = ["bash", "-c"]
  program = ["node", "-e"]

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


7. BRANCH FILTER
-----------------

The `branch` field is a regex matched against the current Git
branch name. The command only runs if the branch matches.

  {
    command.inline = "cargo test --all"
    branch = "main|develop"
  }

  {
    command.inline = "quick-lint"
    branch = "feature/.*"
  }

If the repo is in detached HEAD state (no branch), commands
with a `branch` filter are skipped.

Use this for heavy test suites that should only run on certain
branches, while keeping quick linters on all branches.


8. WORKING DIRECTORY (cwd)
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


9. PARALLEL EXECUTION
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
own file filter, branch, cache, env, cwd, and stdin payload.

If any command fails with severity=error, hoox exits after the
entire batch completes (does not kill sibling threads early).


10. TIMEOUT
-----------

The `timeout` field specifies a maximum duration in seconds.
If the command exceeds this duration, it is killed.

  {
    command.inline = "npm test"
    timeout = 120
  }

On timeout, hoox reports an error message including the
command label and the timeout value. The command is treated
as a failure regardless of severity.

Without `timeout`, commands run indefinitely.


11. RETRY
---------

The `retry` field specifies how many additional attempts to make
if the command fails. Default is 0 (no retries).

  {
    command.inline = "npm audit"
    retry = 3
  }

This runs the command up to 4 times total (1 initial + 3 retries).
On each failure before the final attempt, hoox prints a message:

  hoox: pre-commit:0: failed (attempt 1/4, retrying)

Retries also apply to timeouts — a timed-out command is retried
if attempts remain.

Use this for network-dependent checks (npm audit, license
scanners, external API calls) that may fail transiently.


12. CACHING
-----------

Caching is OPT-IN. Set `cache = true` on a command to enable.

  {
    command.inline = "cargo test"
    files.glob = "**/*.rs"
    cache = true
  }

When enabled, hoox computes a SHA-256 hash of the matched file
paths and their contents. If the hash matches the last successful
run (stored in `.hoox.cache`), the command is skipped.

The cache is stored in `.hoox.cache` at the repo root as a JSON
file. Each entry is keyed by "hook:command_index" (e.g.,
"pre-commit:0").

BEHAVIOR:

  - First run: command executes, hash is saved on success.
  - Subsequent runs: if hash matches, command is skipped.
  - If command fails: cache is NOT updated (stale hash kept).
  - If files change: hash differs, command runs again.

Add `.hoox.cache` to `.gitignore` — it is a local-only file
that should not be committed.

Caching works with all other features (files, branch, parallel,
timeout, env, cwd). The cache check happens AFTER branch and
file matching, so a command is only cached for the specific set
of files that triggered it.


13. STDIN: CHANGED FILES
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

READING IN PYTHON:

  command.inline = """import json, sys
  for f in json.load(sys.stdin):
      print(f"{f['type']}: {f['path']}")"""
  program = ["python3", "-c"]

READING IN NODE:

  command.inline = """
  const files = JSON.parse(require('fs').readFileSync(0,'utf8'));
  files.filter(f => f.type !== 'deleted').forEach(f => console.log(f.path));"""
  program = ["node", "-e"]

NOTE: If stdin is not read by the command, it is harmlessly
ignored. Existing commands that don't need the file list work
without any changes.


14. ENVIRONMENT VARIABLES (env)
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


15. HOOX_CHANGED_FILES
-----------------------

Every command receives the HOOX_CHANGED_FILES environment
variable. It contains a newline-separated list of file paths
relative to the repo root.

  - With a `files` filter: only the files that MATCHED.
  - Without a `files` filter: ALL changed files for this hook.

This is always set, even without `env` configuration.

HOOX_CHANGED_FILES is newline-separated (one file per line).
Stdin (section 12) provides the same list as a JSON array with
type information. Use whichever is more convenient.


16. INCLUDES (MONOREPO)
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


17. VERSION COMPATIBILITY
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


18. HOOK WRAPPER SCRIPTS
-------------------------

`hoox init` installs wrapper scripts in .git/hooks/ for all
19 supported Git hooks. Each contains:

  #!/bin/sh
  hoox run --ignore-missing "${0##*/}" "$@"

The hoox binary must be installed and in PATH for hooks to
execute. If hoox is not found, Git will report a hook failure.

Auto-install via build.rs: if hoox is a Rust build dependency,
hooks are installed during `cargo build` (skipped in CI).


19. AVAILABLE GIT HOOKS
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


20. COMPLETE EXAMPLE
--------------------

version = "0.0.0"
verbosity = all
severity = error

include = ["packages/api/.hoox.conf"]

_shared {
  cargo_check = """set -e
cargo +nightly fmt --all -- --check
cargo test --all"""
}

hooks {
  pre-commit = [
    // Rust: format + test, cached, only on main/develop
    {
      command.inline = ${_shared.cargo_check}
      files.glob = "**/*.rs"
      cache = true
      branch = "main|develop"
    }

    // JS/TS: lint changed files via stdin JSON, in parallel
    {
      command.inline = "cat | jq -r '.[].path' | xargs eslint"
      files.glob = ["**/*.js", "**/*.ts"]
      parallel = true
      timeout = 60
      env.vars { NODE_ENV = "development" }
    }

    // CSS: lint, in parallel with JS
    {
      command.inline = "cat | jq -r '.[].path' | xargs stylelint"
      files.glob = "**/*.css"
      parallel = true
      timeout = 60
    }

    // Python: run in package dir, cached
    {
      command.inline = "pytest"
      cwd = "packages/api"
      files.glob = "packages/api/**/*.py"
      cache = true
      timeout = 300
    }

    // SQL migration check (regex)
    {
      command.inline = "check-migrations"
      files.regex = "db/migrations/.*\\.sql$"
    }

    // Network-dependent check with retry
    {
      command.inline = "npm audit --production"
      cwd = "packages/frontend"
      retry = 2
      timeout = 30
      severity = warn
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
}
