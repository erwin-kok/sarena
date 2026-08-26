use anyhow::Result;
use clap::Parser;
use sarena_api_client::ApiClient;
use sarena_utils::{LoggingConfig, logging};

use crate::{app::App, cli::Cli};

mod app;
mod cli;
mod config;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = config::load_config(&cli)?;

    logging::init_logging(&LoggingConfig {
        enable_debug: config.debug,
        log_file: config.log_file.clone(),
        format: config.log_format,
    });

    let client = ApiClient::new_client(config.host.clone())?;

    let app = App { _config: config, client };

    app.run(&cli.command).await?;

    logging::shutdown_logging();

    Ok(())
}
