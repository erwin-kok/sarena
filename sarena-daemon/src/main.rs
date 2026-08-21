use sarena_api_server::ApiServer;
use sarena_utils::{LoggingConfig, logging};
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;

const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

    let socket_path =
        std::env::var("SARENA_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string());

    info!("starting sarena-daemon, socket = {socket_path}");

    ApiServer::new().start(&socket_path, TCP_PORT).await?;

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
    }

    logging::shutdown_logging();

    Ok(())
}
