use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
};

use crate::config::{
    GIT_HOOK_NAMES,
    HOOX_FILE_NAME,
};

/// Walk up from `cwd` to find the repository root (directory containing
/// `.git`).
pub fn find_repo_root(mut cwd: PathBuf) -> Result<PathBuf> {
    loop {
        if cwd.join(".git").is_dir() {
            return Ok(cwd);
        }
        if !cwd.pop() {
            return Err(anyhow::anyhow!("not a git repository"));
        }
    }
}

/// Create a `.hoox.conf` config file at the repository root if one doesn't
/// exist.
pub fn create_config(repo_path: &Path, template: Option<&str>) -> Result<()> {
    let hoox_path = repo_path.join(HOOX_FILE_NAME);
    if hoox_path.exists() {
        return Ok(());
    }

    let hooks_comment = GIT_HOOK_NAMES.iter().map(|h| format!("//   {}", h)).collect::<Vec<_>>().join("\n");

    let template_content =
        template.unwrap_or("hooks {\n  pre-commit = [\n    { command.inline = \"echo hello\" }\n  ]\n}\n");

    let content = format!(
        "version = \"{}\"\nverbosity = all\n\n// Available Git hooks:\n{}\n\n{}\n",
        env!("CARGO_PKG_VERSION"),
        hooks_comment,
        template_content,
    );

    std::fs::write(&hoox_path, content).context("failed to write .hoox.conf")?;
    Ok(())
}

/// Install hook wrapper scripts in `.git/hooks/` for all supported Git hooks.
/// Each wrapper delegates to `hoox run --ignore-missing`.
pub fn install_hooks(repo_path: &Path) -> Result<()> {
    let hooks_dir = repo_path.join(".git/hooks");
    for hook_name in GIT_HOOK_NAMES {
        let hook_path = hooks_dir.join(hook_name);
        let content = "#!/bin/sh\nhoox run --ignore-missing \"${0##*/}\" \"$@\"\n";
        std::fs::write(&hook_path, content).with_context(|| format!("failed to write hook: {}", hook_name))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))
                .with_context(|| format!("failed to set permissions on hook: {}", hook_name))?;
        }
    }
    Ok(())
}
