# CLAUDE.md

> **Self-updating**: When you learn something new about this project's patterns, conventions,
> architecture, or coding standards during a task, update this file immediately. Keep it concise
> and authoritative — this is the single source of truth for how to work in this codebase.

## Project

hoox is Git hooks on steroids — a declarative Git hook manager that lets you define, version,
and execute hooks via a simple YAML configuration file. Hooks are defined in `.hoox.yaml` at
the repository root and executed through the `hoox` CLI. Single binary, no external dependencies.

**Workspace layout**: `crates/hoox/` is the sole crate. Workspace root at `Cargo.toml`.

```
crates/hoox/src/
  main.rs       Entry point, CLI routing (clap derive)
  args.rs       CLI argument parsing — Cli struct, Command enum, value enums
  config.rs     Configuration schema (.hoox.yaml): Hoox, HookCommand, FilePattern,
                CommandContent, Verbosity, CommandSeverity, constants
  hooks.rs      Hook execution: version checking, command running, changed-file detection,
                glob pattern matching via globset
  init.rs       Repository initialization: find repo root, create .hoox.yaml, install
                hook wrapper scripts in .git/hooks/
  reference.rs  Documentation generation: manpages, markdown, shell completions
```

## Design Principles

1. **Declarative config** — All hooks defined in a single `.hoox.yaml` file. No scattered scripts.
2. **Version-locked** — Config version must be compatible with CLI version. Pre-1.0: minor must
   match. Post-1.0: major must match. Dev builds (0.0.0) accept any config.
3. **Glob-based file matching** — Commands can specify `files` glob patterns to run only when
   matching files have changed. Uses `globset` for fast multi-pattern matching.
4. **YAML anchors** — Reuse command definitions via YAML anchors to avoid repetition.
5. **Flexible execution** — Commands can be inline shell (`!inline`), external files (`!file`),
   or use custom program executors via the `program` field.
6. **Per-command overrides** — Verbosity and severity configurable globally and per-command.
7. **Zero-config hook installation** — `hoox init` writes all 19 Git hooks as thin shell wrappers
   that delegate to `hoox run --ignore-missing`.
8. **Build-time auto-init** — `build.rs` installs hooks during `cargo build` (skipped in CI).

## Configuration Format (.hoox.yaml)

```yaml
version: "0.0.0"
verbosity: all          # [all, none, stdout, stderr]
severity: error         # [error, warn]

# YAML anchors for reuse
.cargo_check: &cargo_check !inline |-
  set -e
  cargo fmt --all -- --check
  cargo test --all

hooks:
  "pre-commit":
    - command: *cargo_check
      files: !glob "**/*.rs"                            # single glob
    - command: !inline 'prettier --check .'
      files: !glob ["**/*.js", "**/*.ts", "**/*.css"]   # multiple globs
    - command: !inline 'check-migrations'
      files: !regex "migrations/.*\\.sql$"              # regex
    - command: !file "./scripts/lint.sh"
      verbosity: stderr
      severity: warn
  "pre-push":
    - command: *cargo_check
```

### File matching

The `files` field uses a tagged enum — `!glob` or `!regex` — following the same YAML tag
pattern as `!inline` / `!file` for commands. Accepts a single pattern or list of patterns.

```yaml
files: !glob "**/*.rs"                    # single glob
files: !glob ["**/*.rs", "**/*.toml"]     # multiple globs
files: !regex "src/.*\\.rs$"              # single regex
files: !regex [".*\\.rs$", ".*test.*"]    # multiple regexes
```

- Commands without `files` always run
- Changed file detection uses libgit2 (no shell-out to `git`):
  - `pre-commit`, `prepare-commit-msg`, `commit-msg`: staged files (index vs HEAD)
  - All other hooks: workdir diff vs HEAD
- Only added/modified/copied/renamed files are considered

### Command types

- `!inline` — Shell command string, passed as argument to the program
- `!file` — Path to a script file (relative to repo root), contents read and passed to program
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
- `hooks.rs` — Hook execution. Reads config, runs commands, handles output and glob matching.
- `init.rs` — Repository setup. Creates config file and hook wrappers.
- `reference.rs` — Documentation generation only.
- `args.rs` — CLI parsing. Clap derive structs only.

### Enum-driven dispatch

Command types (`CommandContent::Inline | File`), file selectors (`FileSelector::Glob | Regex`),
pattern lists (`PatternList::Single | Multiple`), verbosity levels, and severity levels all
use enums for exhaustive matching.

### Patterns to follow

- **Clap derive** for CLI parsing. Add new commands as variants to the `Command` enum.
- **anyhow::Context** on all fallible operations for readable error chains.
- **Exit code forwarding** — When a hook command fails with `severity: error`, exit with
  the command's exit code via `std::process::exit()` so Git sees the correct status.
- **No async** — All operations are synchronous. No tokio runtime needed.

### Style

- Match arms use leading `|` pipes (configured in `rustfmt.toml`).
- Max line width: 120 chars.
- Prefer `&str` / `&'static str` return types for display methods on enums.
- Avoid `unwrap()` — use `unwrap_or`, `unwrap_or_default`, or propagate with `?`.
- Use `anyhow::Context` for error context on all fallible operations.

## Dependencies

- **clap** (derive) — CLI argument parsing, completions, man page generation.
- **serde** + **serde_yaml** — Configuration parsing. YAML anchors resolved by the parser.
- **anyhow** — Error handling with context chains.
- **git2** — libgit2 bindings for changed-file detection (no shell-out to `git`).
- **globset** — Fast glob pattern matching for `!glob` file selectors.
- **regex** — Regex matching for `!regex` file selectors.
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
