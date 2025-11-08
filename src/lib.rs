mod agent;
mod config;
mod error;
mod utils;
mod plugin;
mod grpc;
mod executor;
mod storage;
mod cli;
mod telemetry;

use clap::Parser;
use anyhow::Result;
use crate::telemetry::logging::init_logging;

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
