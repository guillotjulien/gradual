use clap::{Parser, Subcommand};
use std::time::Duration;

mod analyzers;
mod commands;
mod config;
mod events;
mod git;
mod identity;

#[derive(Parser)]
#[command(name = "gradual", about = "Gradual TS/ESLint improvement tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Gate commits: exit 1 if any new findings appear since baseline.
    Check {
        /// Kill analyzers after this many seconds (default: no limit).
        #[arg(long, value_name = "SECS")]
        timeout: Option<u64>,
    },
    /// Record current findings as a new baseline delta event.
    Update {
        /// Accept new findings (regressions) instead of failing.
        #[arg(long)]
        force: bool,
        /// Skip interactive confirmation in non-TTY mode (requires --force).
        #[arg(long)]
        yes: bool,
        /// Kill analyzers after this many seconds (default: no limit).
        #[arg(long, value_name = "SECS")]
        timeout: Option<u64>,
    },
    /// Initialize the baseline from the current finding state.
    Init,
    /// Install the gradual pre-commit hook into .git/hooks/pre-commit.
    InstallHook,
    /// Show current baseline statistics grouped by rule.
    Status,
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { timeout } => {
            commands::check::run(timeout.map(Duration::from_secs))
        }
        Command::Update { force, yes, timeout } => {
            commands::update::run(force, yes, timeout.map(Duration::from_secs))
        }
        Command::Init => commands::init::run(),
        Command::InstallHook => commands::install_hook::run(),
        Command::Status => commands::status::run(),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
