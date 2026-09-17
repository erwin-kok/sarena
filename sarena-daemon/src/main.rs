use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use ipnet::{IpNet, Ipv4Net};
use sarena_control_plane::{ControlPlane, ControlPlaneConfig, ControlPlaneHandle};
use sarena_utils::{LogFormat, TracingConfig, logging};
use tokio::{
    signal::unix::{SignalKind, signal},
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing(&TracingConfig {
        format: LogFormat::Text,
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
    let ControlPlaneHandle {
        state,
        loader_handle,
        loader_thread,
    } = control_plane.start().await?;

    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();

    {
        // API SERVER
        let server_shutdown = shutdown.child_token();
        tasks.spawn(async move {
            sarena_api_server::ApiServer::new()
                .start(&socket_path, TCP_PORT, state, server_shutdown)
                .await
        });
    }

    {
        // Kubernetes controllers
        let token = shutdown.child_token();
        tasks.spawn(async move {
            sarena_kubernetes::controllers::orchestrator::start(token)
                .await
                .map_err(anyhow::Error::from)
        });
    }

    // Wait for either an OS shutdown signal or a task exiting unexpectedly
    // (e.g. panicking, or returning before we asked it to stop). Whichever
    // happens first triggers the same graceful shutdown path below, instead
    // of a crashed task leaving us stuck waiting for a signal that may never
    // come.
    tokio::select! {
        () = wait_for_signal() => {
            info!("shutdown signal received");
        }

        Some(result) = tasks.join_next() => {
            error!("daemon task exited before shutdown was requested");
            log_task_result(result);
        }
    }

    shutdown.cancel();

    let drain = async {
        while let Some(result) = tasks.join_next().await {
            log_task_result(result);
        }
    };
    if tokio::time::timeout(SHUTDOWN_TIMEOUT, drain).await.is_err() {
        error!("timed out waiting for tasks to shut down gracefully");
    }

    loader_handle.shutdown(loader_thread).await?;

    logging::shutdown_tracing();

    Ok(())
}

fn log_task_result(result: Result<anyhow::Result<()>, JoinError>) {
    match result {
        Ok(Ok(())) => info!("daemon task exited"),
        Ok(Err(err)) => error!(?err, "daemon task failed"),
        Err(err) => error!(?err, "daemon task panicked"),
    }
}

async fn wait_for_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("received SIGINT"),
        _ = sigterm.recv() => info!("received SIGTERM"),
    }
}
