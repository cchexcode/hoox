use clap::{
    Parser,
    Subcommand,
    ValueEnum,
};

#[derive(Parser)]
#[command(name = "hoox", version, about = "Git hooks on steroids")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize repository hooks
    Init {
        /// Template to use for the initial .hoox.conf
        #[arg(short, long, default_value = "rust")]
        template: InitTemplate,
    },
    /// Run a hook
    Run {
        /// Hook name (e.g. pre-commit, pre-push)
        hook: String,
        /// Additional arguments passed to hook commands
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
        /// Don't fail if the hook is not defined
        #[arg(long)]
        ignore_missing: bool,
        /// Show what would run without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate .hoox.conf without running anything
    Validate,
    /// Delete .hoox.cache
    Clean,
    /// List configured hooks and their commands
    List,
    /// Print the .hoox.conf format specification and reference
    Spec,
    /// Generate manual pages
    Man {
        /// Output directory
        #[arg(short, long)]
        out: String,
        /// Output format
        #[arg(short, long)]
        format: ManualFormat,
    },
    /// Generate shell completion scripts
    Autocomplete {
        /// Output directory
        #[arg(short, long)]
        out: String,
        /// Target shell
        #[arg(short, long)]
        shell: clap_complete::Shell,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ManualFormat {
    Manpages,
    Markdown,
}

#[derive(Clone, ValueEnum)]
pub enum InitTemplate {
    Rust,
}
