use std::{env::home_dir, path::PathBuf};

use config::{Config as ConfigLoader, Environment, File};
use sarena_data_plane::PIN_ROOT;
use sarena_utils::LogFormat;
use serde::Deserialize;

use crate::cli::Cli;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
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
    PathBuf::from(PIN_ROOT)
}

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let mut builder = ConfigLoader::builder();

    if let Some(path) = &cli.config {
        builder = builder.add_source(File::from(path.clone()));
    } else if let Some(home) = home_dir() {
        let path = ["toml", "json", "yaml", "yml"]
            .iter()
            .map(|ext| home.join(".sarena").join(format!("sarena-daemon.{ext}")))
            .find(|path| path.is_file());
        if let Some(path) = path {
            builder = builder.add_source(File::from(path));
        }
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

    Ok(config)
}
