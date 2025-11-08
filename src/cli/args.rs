use crate::cli::commands::Commands;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "warden", about = "A simple CLI for managing your Docker containers", long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}
