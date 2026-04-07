use std::{
    fs::File,
    io::Write,
    path::Path,
};

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;
use clap_mangen::Man;

use crate::args::Cli;

fn collect_commands() -> Vec<(String, clap::Command)> {
    let mut cmds: Vec<(String, clap::Command)> = Vec::new();
    fn rec_add(path: &str, cmds: &mut Vec<(String, clap::Command)>, parent: &clap::Command) {
        let new_path = &format!("{}-{}", path, parent.get_name());
        cmds.push((new_path.into(), parent.clone()));
        for subc in parent.get_subcommands() {
            rec_add(new_path, cmds, subc);
        }
    }
    rec_add("", &mut cmds, &Cli::command());
    cmds
}

pub fn build_shell_completion(outdir: &Path, shell: Shell) -> Result<()> {
    let mut app = Cli::command();
    clap_complete::generate_to(shell, &mut app, "hoox", outdir)?;
    Ok(())
}

pub fn build_markdown(outdir: &Path) -> Result<()> {
    for cmd in collect_commands() {
        let name = cmd.0.strip_prefix('-').unwrap_or(&cmd.0);
        let file_path = outdir.join(format!("{}.md", name));
        let mut file = File::create(&file_path)?;
        file.write_all(clap_markdown::help_markdown_command(&cmd.1).as_bytes())?;
    }
    Ok(())
}

pub fn build_manpages(outdir: &Path) -> Result<()> {
    for cmd in collect_commands() {
        let name = cmd.0.strip_prefix('-').unwrap_or(&cmd.0);
        let file_path = outdir.join(format!("{}.1", name));
        let mut file = File::create(&file_path)?;
        Man::new(cmd.1).render(&mut file)?;
    }
    Ok(())
}
