use std::path::PathBuf;

const GIT_HOOK_NAMES: [&str; 19] = [
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

fn main() {
    if ci_info::is_ci() {
        return;
    }

    let Ok(dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let mut cwd = PathBuf::from(dir);

    loop {
        if cwd.join(".git").is_dir() {
            break;
        }
        if !cwd.pop() {
            return;
        }
    }

    let hooks_dir = cwd.join(".git/hooks");
    for hook_name in GIT_HOOK_NAMES {
        let hook_path = hooks_dir.join(hook_name);
        let content = "#!/bin/sh\nhoox run --ignore-missing \"${0##*/}\" \"$@\"\n";
        if std::fs::write(&hook_path, content).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
}
