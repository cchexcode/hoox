# hoox

![](hoox.png)

`hoox` is a CLI tool that allows users to manage Git hooks declaratively as part of the repository.

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

If the hook `$HOOK_NAME` is not defined in `.hoox.yaml`, this command will fail. Pass `--ignore-missing` to skip undefined hooks silently.

## Example

```yaml
version: "0.0.0"
verbosity: all

# YAML anchors for reuse
.cargo: &cargo !inline |-
  set -e
  set -u
  cargo +nightly fmt --all -- --check
  cargo test --all

hooks:
  "pre-commit":
    # re-use YAML anchor — only runs when Rust files are staged
    - command: *cargo
      files: !glob "**/*.rs"

    # inline command with output control
    - command: !inline |-
        cargo doc --no-deps
      verbosity: stderr
      severity: warn

    # reference a script file (path relative to repo root)
    - command: !file "./hello_world.sh"

    # custom program executor with glob file matching
    - command: !file "./hello_world.py"
      program: ["python3", "-c"]
      files: !glob "**/*.py"
      verbosity: stderr
      severity: error

    # multiple glob patterns
    - command: !inline 'prettier --check .'
      files: !glob
        - "**/*.js"
        - "**/*.ts"
        - "**/*.css"

    # regex pattern
    - command: !inline 'check-migrations'
      files: !regex "migrations/.*\\.sql$"

    # multiple regex patterns
    - command: !inline 'validate-schema'
      files: !regex
        - "src/schema/.*\\.rs$"
        - ".*\\.graphql$"

  "pre-push":
    - command: *cargo
      files: !glob "**/*.rs"

  "prepare-commit-msg":
    # write to $COMMIT_MSG_FILE ($1) — template commit message for $EDITOR
    # $0 = path to ".hoox.yaml" file in any hook
    - command: !inline |-
        COMMIT_MSG_FILE=$1
        echo "Work in progress" > $COMMIT_MSG_FILE
```

### File matching

The `files` field uses YAML tags to select the matching mode — `!glob` or `!regex`:
```yaml
files: !glob "**/*.rs"                    # single glob
files: !glob ["**/*.rs", "**/*.toml"]     # multiple globs
files: !regex "src/.*\\.rs$"              # single regex
files: !regex [".*\\.rs$", ".*test.*"]    # multiple regexes
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
