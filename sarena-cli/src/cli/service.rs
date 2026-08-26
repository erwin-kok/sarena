use anyhow::Result;
use clap::{Args, Subcommand};
use tracing::info;

use crate::cli::OutputArgs;

#[derive(Args, Debug)]
pub struct ServiceCommand {
    #[command(subcommand)]
    pub command: ServiceCommands,
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    /// List services
    #[command(alias = "ls")]
    List(ListArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Print clustermesh affinity if available
    #[arg(long)]
    pub clustermesh_affinity: bool,

    #[command(flatten)]
    pub output: OutputArgs,
}

pub fn run(service: &ServiceCommand) -> Result<()> {
    match &service.command {
        ServiceCommands::List(args) => list_services(args),
    }
}

fn list_services(_args: &ListArgs) -> Result<()> {
    info!("listing services");

    Ok(())
}
