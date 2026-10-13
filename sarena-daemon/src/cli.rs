use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "sarena-daemon",
    about = "sarena-daemon",
    long_about = "CLI for the local Sarena daemon"
)]
pub struct Cli {
    /// Path to a configuration file to load before applying the flags below.
    /// When omitted, `~/.sarena` is read if it exists. See docs/config.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Emit debug-level log messages (overrides `debug` from the config file).
    #[arg(short = 'D', long, global = true)]
    pub debug: bool,

    /// Write logs to this file instead of stderr (overrides `log_file` from the
    /// config file).
    #[arg(short = 'L', long, global = true)]
    pub log_file: Option<String>,

    /// Log output format: `text` (one line per event, for a terminal) or `json`
    /// (structured, for production / log aggregation).
    /// Overrides `log_format` from the config file. Default: `text`.
    #[arg(short = 'F', long, global = true, value_parser = ["text", "json"])]
    pub log_format: Option<String>,

    /// Path to cmdref output directory.
    #[arg(long, global = true)]
    pub cmd_ref: Option<PathBuf>,
}
