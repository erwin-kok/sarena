use anyhow::Result;
use clap::{Args, Subcommand};

use crate::{app::App, cli::bpf::endpoint::BpfEndpointCommand};

pub mod endpoint;

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
}

pub fn run(app: &App, command: &BpfCommand) -> Result<()> {
    match &command.command {
        BpfCommands::Endpoint(endpoint) => endpoint::run(app, endpoint),
    }
}
