use anyhow::Result;
use clap::{Args, Subcommand};

use crate::{
    app::App,
    cli::bpf::{endpoint::BpfEndpointCommand, metrics::BpfMetricsCommand},
};

pub mod endpoint;
pub mod metrics;

#[derive(Args, Debug)]
pub struct BpfCommand {
    #[command(subcommand)]
    pub command: BpfCommands,
}

#[derive(Subcommand, Debug)]
pub enum BpfCommands {
    /// Local endpoint map
    #[command(alias = "ep")]
    Endpoint(BpfEndpointCommand),

    /// Traffic metrics
    Metrics(BpfMetricsCommand),
}

pub fn run(app: &App, command: &BpfCommand) -> Result<()> {
    match &command.command {
        BpfCommands::Endpoint(command) => endpoint::run(app, command),
        BpfCommands::Metrics(command) => metrics::run(app, command),
    }
}
