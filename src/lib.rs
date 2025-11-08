mod agent;
mod cli;
mod collector;
mod config;
mod error;
mod executor;
mod grpc;
mod plugin;
mod storage;
mod telemetry;
mod utils;

use crate::telemetry::logging::init_logging;
use anyhow::Result;
use clap::Parser;

pub fn run() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Run(run_cmd) => {
            run_cmd.execute();
        }
    }
    let cfg = config::global();
    let _logger = init_logging(&cfg.telemetry);
    tracing::info!("Warden CLI started");
    Ok(())
}
