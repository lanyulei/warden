use clap::Parser;
use crate::cli::commands::Commands;

#[derive(Debug, Parser)]
#[command(name = "warden", about = "A simple CLI for managing your Docker containers", long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}