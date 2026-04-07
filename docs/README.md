# hoox

![](hoox.png)

`hoox` is a CLI tool that allows users to manage Git hooks declaratively as part of the repository. First-class monorepo support.

## Workflow

### CLI install

The Git hooks contain calls to the `hoox` CLI, so it must be installed for hooks to execute. If not installed, hooks will fail and prevent the operation (by default).

### Repo initialization

In order to initialize a repo you can either:

- Add hoox to the dev-dependencies of a Rust crate you're working with:
  ```bash
  cargo add hoox --dev
  ```
  This installs hooks during the build process (via `build.rs`) even when hoox is not in the root `Cargo.toml`. It walks up the directory tree from the `OUT_DIR` env variable to find the `.git` folder.

- OR install hooks manually:
  ```bash
  hoox init
  ```

### Run hooks manually

```bash
hoox run $HOOK_NAME
```

If the hook `$HOOK_NAME` is not defined in `.hoox.conf`, this command will fail. Pass `--ignore-missing` to skip undefined hooks silently.

## Example

```hocon
version = "0.0.0"
verbosity = all

// HOCON substitutions for reuse
_shared {
  cargo = """set -e
set -u
cargo +nightly fmt --all -- --check
cargo test --all"""
}

hooks {
  pre-commit = [
    // re-use substitution — only runs when Rust files are staged
    {
      command.inline = ${_shared.cargo}
      files.glob = "**/*.rs"
    }

    // inline command with output control
    {
      command.inline = "cargo doc --no-deps"
      verbosity = stderr
      severity = warn
    }

    // reference a script file (path relative to repo root)
    { command.file = "./hello_world.sh" }

    // custom program executor with glob file matching
    {
      command.file = "./hello_world.py"
      program = ["python3", "-c"]
      files.glob = "**/*.py"
      verbosity = stderr
      severity = error
    }

    // multiple glob patterns
    {
      command.inline = "prettier --check ."
      files.glob = ["**/*.js", "**/*.ts", "**/*.css"]
    }

    // regex pattern
    {
      command.inline = "check-migrations"
      files.regex = "migrations/.*\\.sql$"
    }

    // both glob and regex (OR: runs if either matches)
    {
      command.inline = "validate-schema"
      files { glob = "**/*.graphql", regex = "src/schema/.*\\.rs$" }
    }
  ]

  pre-push = [
    {
      command.inline = ${_shared.cargo}
      files.glob = "**/*.rs"
    }
  ]

  prepare-commit-msg = [
    {
      command.inline = """
COMMIT_MSG_FILE=$1
echo "Work in progress" > $COMMIT_MSG_FILE"""
    }
  ]
}
```

## Monorepo support

### Include

Import hook definitions from per-package config files:

```hocon
version = "0.0.0"
include = ["crates/api/.hoox.conf", "packages/web/.hoox.conf"]
hooks { }
```

Included files are full `.hoox.conf` files. Their hooks are appended to the root's hook lists.

### Working directory (`cwd`)

Run a command in a specific directory (relative to repo root):

```hocon
{
  command.inline = "cargo test"
  cwd = "crates/api"
  files.glob = "crates/api/**/*.rs"
}
```

### Parallel execution

Consecutive commands with `parallel = true` run concurrently:

```hocon
hooks {
  pre-commit = [
    {
      command.inline = "cargo test"
      cwd = "crates/api"
      files.glob = "crates/api/**"
      parallel = true
    }
    {
      command.inline = "npm test"
      cwd = "packages/web"
      files.glob = "packages/web/**"
      parallel = true
    }
    // This runs after both parallel commands complete
    { command.inline = "echo done" }
  ]
}
```

### Stdin: changed files

Every command receives matched changed files as a JSON array on stdin.
Each entry has `path` and `type` (`added`, `modified`, `deleted`, `renamed`, `copied`):

```json
[{"path":"src/app.ts","type":"modified"},{"path":"src/new.ts","type":"added"}]
```

Commands that don't read stdin are unaffected. Use with `jq` or any JSON parser:

```hocon
{
  command.inline = "cat | jq -r '.[].path' | xargs prettier --check"
  files.glob = ["**/*.js", "**/*.ts"]
}
```

### Environment variables

```hocon
{
  command.inline = "npm test"
  cwd = "packages/web"
  env {
    // Only keep env vars matching these regex patterns (clean env otherwise)
    keep = ["PATH", "HOME", "NODE_.*", "NPM_.*"]
    // Additional vars to set
    vars { NODE_ENV = "test", CI = "true" }
  }
}
```

- `keep`: regex patterns for env var names to preserve. When set, the command starts with a clean env.
- `vars`: additional env vars to set on top.
- `HOOX_CHANGED_FILES` is always set (newline-separated list of matched files).

### File matching

The `files` field supports `glob` and `regex` matching:
```hocon
files.glob = "**/*.rs"                    // single glob
files.glob = ["**/*.rs", "**/*.toml"]     // multiple globs
files.regex = "src/.*\\.rs$"              // single regex
files.regex = [".*\\.rs$", ".*test.*"]    // multiple regexes
files { glob = "**/*.rs", regex = ".*test.*" }  // both (OR logic)
```

Changed file detection (via libgit2, no shell-out):
- For `pre-commit`, `prepare-commit-msg`, `commit-msg`: staged files (index vs HEAD)
- For all other hooks: workdir diff vs HEAD
- Commands without `files` always run

### Available hooks

- `applypatch-msg`
- `commit-msg`
- `post-applypatch`
- `post-checkout`
- `post-commit`
- `post-merge`
- `post-receive`
- `post-rewrite`
- `post-update`
- `pre-applypatch`
- `pre-auto-gc`
- `pre-commit`
- `pre-push`
- `pre-rebase`
- `pre-receive`
- `prepare-commit-msg`
- `push-to-checkout`
- `sendemail-validate`
- `update`
