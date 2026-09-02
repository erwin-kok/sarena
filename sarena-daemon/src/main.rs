use std::net::{IpAddr, Ipv4Addr};

use ipnet::{IpNet, Ipv4Net};
use sarena_api_server::ApiServer;
use sarena_control_plane::{ControlPlane, ControlPlaneConfig};
use sarena_utils::{LogFormat, LoggingConfig, logging};
use tokio::signal::unix::{SignalKind, signal};
use tracing::info;

const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_logging(&LoggingConfig {
        format: LogFormat::Json,
        ..Default::default()
    });

    let socket_path =
        std::env::var("SARENA_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string());

    info!("starting sarena-daemon, socket = {socket_path}");

    let config = ControlPlaneConfig {
        gateway_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
        internal_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
        ipam_ipv4_subnet: Some(IpNet::V4(Ipv4Net::new(Ipv4Addr::new(10, 0, 10, 0), 24)?)),
        ipam_ipv6_subnet: None,
    };

    let control_plane = ControlPlane::new(config);
    let state = control_plane.start().await?;

    ApiServer::new()
        .start(&socket_path, TCP_PORT, state)
        .await?;

    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("received SIGINT, shutting down"),
        _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
    }

    logging::shutdown_logging();

    Ok(())
}
