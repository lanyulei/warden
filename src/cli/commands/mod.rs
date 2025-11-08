mod run;

use clap::Subcommand;
use run::Run;

#[derive(Debug, Subcommand)]
pub enum Commands {
    Run(Run),
}
