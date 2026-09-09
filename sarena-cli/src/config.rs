use std::{env::home_dir, path::PathBuf};

use config::{Config as ConfigLoader, Environment, File};
use sarena_utils::LogFormat;
use serde::Deserialize;

use crate::cli::Cli;

const DEFAULT_PIN_ROOT: &str = "/sys/fs/bpf/sarena";

/// Effective CLI configuration, merged from (lowest to highest precedence):
/// the config file (`--config <path>`, or `~/.sarena` when it exists),
/// `SARENA_*` environment variables, and finally the command-line flags.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Address of the daemon, as `unix://<path>` or `tcp://<host:port>`.
    /// When unset the client falls back to `unix:///tmp/sarena.sock`.
    pub host: Option<String>,

    /// Emit debug-level log messages.
    #[serde(default)]
    pub debug: bool,

    /// Write logs to this file instead of stderr.
    #[serde(default)]
    pub log_file: Option<String>,

    /// Log output format: `text` (default) or `json`.
    #[serde(default)]
    pub log_format: LogFormat,

    /// BPF filesystem directory holding the datapath's pinned maps, opened by
    /// the `bpf` subcommands. Default: `/sys/fs/bpf/sarena`.
    #[serde(default = "default_pin_root")]
    pub pin_root: PathBuf,
}

fn default_pin_root() -> PathBuf {
    PathBuf::from(DEFAULT_PIN_ROOT)
}

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let mut builder = ConfigLoader::builder();

    if let Some(path) = &cli.config {
        builder = builder.add_source(File::from(path.clone()));
    } else if let Some(home) = home_dir() {
        let path = home.join(".sarena");
        builder = builder.add_source(File::from(path).required(false));
    }

    builder = builder.add_source(Environment::with_prefix("SARENA").separator("_"));

    let mut config: Config = builder.build()?.try_deserialize()?;

    if cli.debug {
        config.debug = true;
    }

    if cli.log_file.is_some() {
        config.log_file.clone_from(&cli.log_file);
    }

    if let Some(log_format) = &cli.log_format {
        config.log_format = log_format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    }

    if cli.host.is_some() {
        config.host.clone_from(&cli.host);
    }

    Ok(config)
}
