mod args;
mod config;
mod hooks;
mod init;
mod reference;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = args::Cli::parse();

    match cli.command {
        | args::Command::Init { template } => {
            let repo = init::find_repo_root(std::env::current_dir()?)?;
            let content = match template {
                | args::InitTemplate::Rust => include_str!("../res/templates/rust.conf"),
            };
            init::create_config(&repo, Some(content))?;
            init::install_hooks(&repo)?;
        },
        | args::Command::Run {
            hook,
            args,
            ignore_missing,
        } => {
            hooks::run(&hook, &args, ignore_missing)?;
        },
        | args::Command::Spec => {
            print!("{}", include_str!("../res/SPEC.md"));
        },
        | args::Command::Man { out, format } => {
            let out_path = PathBuf::from(out);
            std::fs::create_dir_all(&out_path)?;
            match format {
                | args::ManualFormat::Manpages => reference::build_manpages(&out_path)?,
                | args::ManualFormat::Markdown => reference::build_markdown(&out_path)?,
            }
        },
        | args::Command::Autocomplete { out, shell } => {
            let out_path = PathBuf::from(out);
            std::fs::create_dir_all(&out_path)?;
            reference::build_shell_completion(&out_path, shell)?;
        },
    }

    Ok(())
}
