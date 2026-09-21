use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr},
    process,
    time::Duration,
};

use ipnet::{IpNet, Ipv4Net};
use sarena_control_plane::{ControlPlane, ControlPlaneConfig, ControlPlaneHandle};
use sarena_utils::{LogFormat, TracingConfig, logging, metrics::init_metrics};
use tokio::{
    signal::unix::{SignalKind, signal},
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const TELEMETRY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init_tracing(&TracingConfig {
        format: LogFormat::Text,
        ..Default::default()
    });

    let metrics_state = init_metrics()?;

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
        let metrics_registry = metrics_state.registry.clone();
        tasks.spawn(async move {
            sarena_api_server::ApiServer::new()
                .start(
                    &socket_path,
                    TCP_PORT,
                    state,
                    metrics_registry,
                    server_shutdown,
                )
                .await
        });
    }

    {
        // Kubernetes controllers
        let token = shutdown.child_token();
        let metrics_registry = metrics_state.registry.clone();
        tasks.spawn(async move {
            sarena_kubernetes::server::start(metrics_registry, token)
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

    shutdown_step("daemon tasks", SHUTDOWN_TIMEOUT, async {
        while let Some(result) = tasks.join_next().await {
            log_task_result(result);
        }
    })
    .await;

    shutdown_step("eBPF loader", SHUTDOWN_TIMEOUT, async {
        if let Err(err) = loader_handle.shutdown(loader_thread).await {
            error!(?err, "failed to shut down eBPF loader");
        }
    })
    .await;

    shutdown_step(
        "OTel meter provider",
        TELEMETRY_SHUTDOWN_TIMEOUT,
        shutdown_blocking("OTel meter provider", move || {
            if let Err(err) = metrics_state.provider.shutdown() {
                error!(?err, "failed to shut down OTel meter provider");
            }
        }),
    )
    .await;

    shutdown_step(
        "OTel tracer provider",
        TELEMETRY_SHUTDOWN_TIMEOUT,
        shutdown_blocking("OTel tracer provider", logging::shutdown_tracing),
    )
    .await;

    process::exit(0)
}

async fn shutdown_step(name: &str, timeout: Duration, fut: impl Future<Output = ()>) {
    if tokio::time::timeout(timeout, fut).await.is_err() {
        error!("timed out shutting down {name}");
    }
}

async fn shutdown_blocking(name: &str, f: impl FnOnce() + Send + 'static) {
    if tokio::task::spawn_blocking(f).await.is_err() {
        error!("{name} shutdown task panicked");
    }
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
