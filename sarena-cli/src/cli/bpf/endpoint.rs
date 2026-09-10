use std::{
    collections::BTreeMap,
    io::{self, Write as _},
    net::Ipv4Addr,
};

use anyhow::{Context as _, Result};
use clap::{Args, Subcommand};
use sarena_loader::{LxcMap, PinRoot};
use tabwriter::TabWriter;
use tracing::info;

use crate::{
    app::App,
    cli::{OutputArgs, OutputFormat},
    output::print_output,
    utils,
};

#[derive(Args, Debug)]
pub struct BpfEndpointCommand {
    #[command(subcommand)]
    pub command: BpfEndpointCommands,
}

#[derive(Subcommand, Debug)]
pub enum BpfEndpointCommands {
    /// List local endpoint entries
    #[command(alias = "ls")]
    List(OutputArgs),

    /// Delete a local endpoint entry
    Delete(DeleteArgs),
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// IPv4 address of the endpoint to remove
    pub ip: Ipv4Addr,
}

pub fn run(app: &App, command: &BpfEndpointCommand) -> Result<()> {
    match &command.command {
        BpfEndpointCommands::List(args) => list_endpoints(app, args.output.as_ref()),
        BpfEndpointCommands::Delete(args) => delete_endpoint(app, args.ip),
    }
}

fn list_endpoints(app: &App, format: Option<&OutputFormat>) -> Result<()> {
    utils::require_root_privilege();

    let pins = PinRoot::new(app.config.pin_root.clone());
    let map = LxcMap::open(&pins).context("opening lxc_map (is the datapath loaded?)")?;
    let endpoints = map.endpoints().context("iterating lxc_map")?;
    let endpoints: BTreeMap<String, String> = endpoints
        .into_iter()
        .map(|(ip, info)| (ip.to_string(), info.to_string()))
        .collect();

    if let Some(f) = format {
        print_output(&endpoints, f)
    } else {
        print_table(&endpoints)
    }
}

fn delete_endpoint(app: &App, ip: Ipv4Addr) -> Result<()> {
    utils::require_root_privilege();

    let pins = PinRoot::new(app.config.pin_root.clone());
    let mut map = LxcMap::open(&pins).context("opening lxc_map (is the datapath loaded?)")?;
    map.remove_endpoint(ip)
        .context("removing endpoint from lxc_map")?;
    info!(%ip, "deleted local endpoint");
    Ok(())
}

fn print_table(endpoints: &BTreeMap<String, String>) -> Result<()> {
    let mut tw = TabWriter::new(io::stdout());
    writeln!(tw, "IP-ADDRESS\tLOCAL-ENDPOINT-INFO")?;
    for (ip, info) in endpoints {
        writeln!(tw, "{ip}\t{info}")?;
    }
    tw.flush()?;

    Ok(())
}
