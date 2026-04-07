# CLAUDE.md

> **Self-updating**: When you learn something new about this project's patterns, conventions,
> architecture, or coding standards during a task, update this file immediately. Keep it concise
> and authoritative — this is the single source of truth for how to work in this codebase.

## Project

hoox is Git hooks on steroids — a declarative Git hook manager that lets you define, version,
and execute hooks via a HOCON configuration file. Hooks are defined in `.hoox.conf` at the
repository root and executed through the `hoox` CLI. Single binary, no external dependencies.
First-class monorepo support via `include`, `cwd`, `parallel`, and stdin file piping.

**Workspace layout**: `crates/hoox/` is the sole crate. Workspace root at `Cargo.toml`.

```
crates/hoox/src/
  main.rs       Entry point, CLI routing (clap derive)
  args.rs       CLI argument parsing — Cli struct, Command enum, value enums
  config.rs     Configuration schema (.hoox.conf): Hoox, HookCommand, CommandSpec,
                FileSelector, PatternList, EnvConfig, Verbosity, CommandSeverity, constants
  hooks.rs      Hook execution: include resolution, version checking, command batching,
                parallel execution (std::thread::scope), file matching, env building
  init.rs       Repository initialization: find repo root, create .hoox.conf, install
                hook wrapper scripts in .git/hooks/
  reference.rs  Documentation generation: manpages, markdown, shell completions
```

## Design Principles

1. **Declarative config** — All hooks defined in `.hoox.conf` (HOCON). No scattered scripts.
2. **Version-locked** — Config version must be compatible with CLI version. Pre-1.0: minor must
   match. Post-1.0: major must match. Dev builds (0.0.0) accept any config.
3. **File matching** — Commands can specify `files.glob` and/or `files.regex` patterns to run
   only when matching files have changed. Both can be set (OR logic).
4. **HOCON substitutions** — Reuse command definitions via `${}` substitutions.
5. **Flexible execution** — Commands use `command.inline` for shell strings or `command.file`
   for script paths, with optional `program` executor.
6. **Per-command overrides** — Verbosity, severity, env, and cwd configurable per-command.
7. **Monorepo-native** — `include` for per-package configs, `cwd` for package dirs,
   `parallel` for concurrent execution, stdin JSON piping for targeted linting.
8. **Zero-config hook installation** — `hoox init` writes all 19 Git hooks as thin shell wrappers.
9. **Build-time auto-init** — `build.rs` installs hooks during `cargo build` (skipped in CI).

## Configuration Format (.hoox.conf)

```hocon
version = "0.0.0"
verbosity = all          // all, none, stdout, stderr
severity = error         // error, warn

// Include per-package configs (paths relative to repo root)
include = ["crates/api/.hoox.conf", "packages/web/.hoox.conf"]

// HOCON substitutions for reuse
_shared {
  cargo_check = """set -e
cargo fmt --all -- --check
cargo test --all"""
}

hooks {
  pre-commit = [
    // Run in a specific directory, only when matching files change
    {
      command.inline = ${_shared.cargo_check}
      cwd = "crates/api"
      files.glob = "crates/api/**/*.rs"
    }
    // Run in parallel with the next command
    {
      command.inline = "cargo test"
      cwd = "crates/api"
      files.glob = "crates/api/**/*.rs"
      parallel = true
    }
    {
      command.inline = "npm test"
      cwd = "packages/web"
      files.glob = "packages/web/**"
      parallel = true
    }
    // Read matched files from stdin JSON (path + type) + custom env
    {
      command.inline = "cat | jq -r '.[].path' | xargs prettier --check"
      files.glob = ["**/*.js", "**/*.ts", "**/*.css"]
      env {
        keep = ["PATH", "HOME", "NODE_.*"]
        vars { NODE_ENV = "production" }
      }
    }
    // Regex file matching
    {
      command.inline = "check-migrations"
      files.regex = "migrations/.*\\.sql$"
    }
    // Script file with custom executor
    {
      command.file = "./scripts/lint.sh"
      verbosity = stderr
      severity = warn
    }
  ]
}
```

### Include

The `include` field lists paths to additional `.hoox.conf` files (relative to repo root).
Their hooks are appended to the root config's hook lists. Included files can use their own
HOCON substitutions. Nested includes (include within include) are not processed.

### File matching

The `files` field is a struct with optional `glob` and `regex` fields.
Both accept a single pattern (string) or list of patterns (array).

```hocon
files.glob = "**/*.rs"                    // single glob
files.glob = ["**/*.rs", "**/*.toml"]     // multiple globs
files.regex = "src/.*\\.rs$"              // single regex
files { glob = "**/*.rs", regex = ".*test.*" }  // both (OR)
```

- Commands without `files` always run
- If both `glob` and `regex` are set, the command runs if either matches (OR)
- Changed file detection uses libgit2 (no shell-out to `git`):
  - `pre-commit`, `prepare-commit-msg`, `commit-msg`: staged files (index vs HEAD)
  - All other hooks: workdir diff vs HEAD
- Only added/modified/copied/renamed files are considered

### Parallel execution

Consecutive commands with `parallel = true` are grouped and run concurrently via
`std::thread::scope`. Commands without `parallel` (or `parallel = false`) are sequential
barriers — they run alone, in order.

```
cmd A (sequential)    →  runs alone
cmd B (parallel=true) ─┐
cmd C (parallel=true) ─┤  run concurrently
cmd D (parallel=true) ─┘
cmd E (sequential)    →  runs alone after B/C/D
```

Output from parallel commands is printed as it arrives (no buffering). If any command in a
parallel batch fails with `severity = error`, the process exits after the batch completes.

### Stdin: changed files

Every command receives its matched changed files as a JSON array piped to stdin.
Each entry has `path` and `type` (git2 Delta variants, lowercased:
`added`, `modified`, `deleted`, `renamed`, `copied`).
Commands that don't read stdin are unaffected.

```json
[{"path":"src/main.rs","type":"modified"},{"path":"src/new.rs","type":"added"}]
```

```hocon
{ command.inline = "cat | jq -r '.[].path' | xargs prettier --check" }
```

### Environment variables

The `env` field configures the command's environment:

```hocon
env {
  keep = ["PATH", "HOME", "RUST_.*", "CARGO_.*"]  // regex patterns
  vars { RUST_LOG = "debug", CI = "true" }
}
```

- `keep`: regex patterns for env var names to preserve. When set, the command starts with
  a clean environment and only inherits vars whose names match at least one pattern.
  When absent, the full environment is inherited.
- `vars`: additional env vars set on top (always applied).
- `HOOX_CHANGED_FILES` is always set (newline-separated matched files).

### Command types

- `command.inline` — Shell command string, passed as argument to the program
- `command.file` — Path to a script file (relative to repo root)
- `program` — Optional custom executor (default: `["sh", "-c"]`)

### Hook wrapper scripts

Each `.git/hooks/<name>` contains:
```sh
#!/bin/sh
hoox run --ignore-missing "${0##*/}" "$@"
```

## CLI Commands

```
hoox init [-t rust]                    Initialize repo hooks
hoox run <hook> [args...] [--ignore-missing]   Execute a hook
hoox man -o <path> -f <manpages|markdown>      Generate docs
hoox autocomplete -o <path> -s <shell>         Generate completions
```

## Coding Standards

### Object-oriented style

All behavior lives in `impl` blocks on the struct that owns the relevant state. Module-level
functions are fine for stateless operations (e.g., `find_repo_root`, `check_version`).

### Module structure

Each module has a single clear responsibility:
- `config.rs` — Data types and constants only. No execution logic.
- `hooks.rs` — Hook execution: include resolution, batching, parallel dispatch, env setup.
- `init.rs` — Repository setup. Creates config file and hook wrappers.
- `reference.rs` — Documentation generation only.
- `args.rs` — CLI parsing. Clap derive structs only.

### Struct and enum patterns

- `CommandSpec` — struct with optional `inline`/`file` fields (exactly one must be set,
  validated at runtime). Struct used instead of enum because HOCON crate doesn't support
  serde's externally tagged enums.
- `FileSelector` — struct with optional `glob`/`regex` fields (both can be set for OR logic).
- `PatternList` — untagged enum (`Single(String)` / `Multiple(Vec<String>)`) for flexible
  single-or-array pattern syntax.
- `EnvConfig` — struct with optional `vars` (HashMap) and `keep` (Vec of regex patterns).
- `Batch` — internal enum for command grouping (`Sequential` / `Parallel`).
- Verbosity and severity use `rename_all = "snake_case"` enums.

### Patterns to follow

- **Clap derive** for CLI parsing. Add new commands as variants to the `Command` enum.
- **hocon::de::from_str** for config parsing. HOCON is the config format.
- **anyhow::Context** on all fallible operations for readable error chains.
- **Exit code forwarding** — When a hook command fails with `severity = error`, exit with
  the command's exit code via `std::process::exit()` so Git sees the correct status.
- **std::thread::scope** for parallel command execution. No async runtime.
- **execute_command** returns `Result<Option<i32>>` — `None` = skipped/success,
  `Some(code)` = failed. Caller handles exit.

### Style

- Match arms use leading `|` pipes (configured in `rustfmt.toml`).
- Max line width: 120 chars.
- Prefer `&str` / `&'static str` return types for display methods on enums.
- Avoid `unwrap()` — use `unwrap_or`, `unwrap_or_default`, or propagate with `?`.
- Use `anyhow::Context` for error context on all fallible operations.

## Dependencies

- **clap** (derive) — CLI argument parsing, completions, man page generation.
- **hocon** — HOCON configuration parsing via serde. Supports substitutions (`${}`).
- **serde** — Serialization/deserialization framework.
- **anyhow** — Error handling with context chains.
- **git2** — libgit2 bindings for changed-file detection (no shell-out to `git`).
- **globset** — Fast glob pattern matching for `files.glob` selectors.
- **regex** — Regex matching for `files.regex` and `env.keep` patterns.
- **ci_info** (build only) — CI environment detection for build.rs.

## Build

```sh
cargo build -p hoox              # debug
cargo build --release -p hoox    # release
cargo run -p hoox                # run
cargo run -p hoox -- init        # initialize hooks
cargo run -p hoox -- run pre-commit
```

## Formatting

**Always run after every edit session:**

```sh
cargo +nightly fmt
```

This formats the entire workspace. Never skip this step — all code must be formatted before
committing or reviewing.
