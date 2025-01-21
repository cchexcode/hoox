# hoox

![](hoox.png)

`hoox` is an application / library that allows users to manage git hooks as part of the repository.

## Workflow

### CLI install

The Git hooks will contain calls to the `hoox` cli, therefore making it necessary that the `hoox` CLI is installed in order to execute the Git hooks. If it is not installed, the hooks will fail and prevent the operation (by default).

### Repo initialization

In order to initialize a repo you can either:

- Add hoox to the dev-dependencies of the crate you're working with (if you're working on a rust project)
  ```bash
  cargo add hoox --dev
  ```
  This command installs hoox in the git repository during the build process (using a custom `build.rs`) even when it's not in the root `Cargo.toml`. It moves up the directory path, starting from the `OUT_DIR` env variable during build (usually the `target` folder), to find the first folder containing a `.git` subfolder.
- OR install hoox manually into the Git folder with
  ```bash
  hoox init
  ```
  This method works the same way as the method mentioned above although it does not use the `OUT_DIR` env variable that is present during build but it uses the current working directory of the shell (`cwd`).

### Run hooks manually

To run hooks manually, use:

```bash
hoox run $HOOK_NAME
```

If the hook `$HOOK_NAME` was _not_ specified in the `.hoox.toml` file, this command will fail. In order to make the command succeed and ignore the missing hook definition, pass the `--ignore-missing` parameter.

## Example

```yaml
version: "0.0.0"
verbosity: all

.cargo: &cargo !inline |-
  set -e
  set -u
  cargo +nightly fmt --all -- --check
  cargo test --all

hooks:
  "pre-commit":
    # use YAML anchor
    - command: *cargo
    # use inline command
    - command: !inline |-
        cargo doc --no-deps
      verbosity: stderr
      severity: warn
    # reference a script file (path is relative to Git repo root)
    - command: !file "./hello_world.sh"
    # reference a script file with custom program
    - command: !file "./hello_world.py"
      program: ["python3", "-c"]
      verbosity: stderr
      severity: error

  "pre-push":
    # re-use YAML anchor
    - command: *cargo

  "prepare-commit-msg":
    # write to $COMMIT_MSG_FILE ($1) which is going to be the template commit message for this commit
    # which is subsequently opened in the $EDITOR program.
    # $0 = path to "hoox.yaml" file in any hook
    - command: !inline |-
        COMMIT_MSG_FILE=$1
        echo "Work in progress" > $COMMIT_MSG_FILE
```

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
