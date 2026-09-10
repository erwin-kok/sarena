use std::io::{self, Write as _};

use anyhow::{Context as _, Result};
use clap::{Args, Subcommand};
use sarena_loader::{PinRoot, maps::MetricsMap};
use sarena_shared::write_obs_point;
use serde::Serialize;
use tabwriter::TabWriter;
use tracing::info;

use crate::{
    app::App,
    cli::{OutputArgs, OutputFormat},
    output::print_output,
    utils,
};

#[derive(Args, Debug)]
pub struct BpfMetricsCommand {
    #[command(subcommand)]
    pub command: BpfMetricsCommands,
}

#[derive(Subcommand, Debug)]
pub enum BpfMetricsCommands {
    /// List datapath traffic metrics
    #[command(alias = "ls")]
    List(OutputArgs),

    /// Clear datapath traffic metrics
    Flush,
}

#[derive(Serialize)]
struct MetricRow {
    obs_point: &'static str,
    packets: u64,
    bytes: u64,
}

pub fn run(app: &App, command: &BpfMetricsCommand) -> Result<()> {
    match &command.command {
        BpfMetricsCommands::List(args) => list_metrics(app, args.output.as_ref()),
        BpfMetricsCommands::Flush => flush_metrics(app),
    }
}

fn list_metrics(app: &App, format: Option<&OutputFormat>) -> Result<()> {
    utils::require_root_privilege();

    let pins = PinRoot::new(app.config.pin_root.clone());
    let map = MetricsMap::open(&pins).context("opening metrics_map (is the datapath loaded?)")?;

    let mut rows: Vec<MetricRow> = map
        .metrics()
        .map(|res| {
            let (key, value) = res.context("iterating metrics_map")?;
            Ok(MetricRow {
                obs_point: write_obs_point(key.obs_point),
                packets: value.packets,
                bytes: value.bytes,
            })
        })
        .collect::<Result<_>>()?;
    rows.sort_unstable_by_key(|r| r.obs_point);

    if let Some(f) = format {
        print_output(&rows, f)
    } else {
        print_table(&rows)
    }
}

fn flush_metrics(app: &App) -> Result<()> {
    utils::require_root_privilege();

    let pins = PinRoot::new(app.config.pin_root.clone());
    let mut map =
        MetricsMap::open(&pins).context("opening metrics_map (is the datapath loaded?)")?;
    map.clear().context("clearing metrics_map")?;
    info!("cleared datapath traffic metrics");

    Ok(())
}

fn print_table(rows: &[MetricRow]) -> Result<()> {
    let mut tw = TabWriter::new(io::stdout());
    writeln!(tw, "OBS-POINT\tPACKETS\tBYTES")?;
    for row in rows {
        writeln!(tw, "{}\t{}\t{}", row.obs_point, row.packets, row.bytes)?;
    }
    tw.flush()?;

    Ok(())
}
