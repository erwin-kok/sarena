use std::env::home_dir;

use config::{Config as ConfigLoader, Environment, File};
use sarena_utils::LogFormat;
use serde::Deserialize;

use crate::cli::Cli;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub host: Option<String>,

    #[serde(default)]
    pub debug: bool,

    #[serde(default)]
    pub log_file: Option<String>,

    #[serde(default)]
    pub log_format: LogFormat,
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
