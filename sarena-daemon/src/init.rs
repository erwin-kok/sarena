use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
    time::Duration,
};

use ipnet::{IpNet, Ipv4Net};
use sarena_data_plane::{DataPlane as _, DataPlaneConfig, DefaultDataPlane, EndpointManager};
use sarena_services::{AppState, DefaultDaemonService, DefaultEndpointService, DefaultIpamService};
use sarena_utils::{logging, metrics::init_metrics};
use tokio::{
    task::{Id, JoinError, JoinSet},
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::Config;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const TELEMETRY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(25);
const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

pub async fn run_daemon(_config: &Config, shutdown: &CancellationToken) -> anyhow::Result<()> {
    let metrics_state = init_metrics()?;

    let socket_path =
        std::env::var("SARENA_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string());

    info!("starting sarena-daemon, socket = {socket_path}");

    let config = DataPlaneConfig {
        gateway_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
        internal_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
        ipam_ipv4_subnet: Some(IpNet::V4(Ipv4Net::new(Ipv4Addr::new(10, 0, 10, 0), 24)?)),
        ipam_ipv6_subnet: None,
    };

    let data_plane = DefaultDataPlane::new(&config).await?;

    let endpoint_manager = EndpointManager::new(data_plane.loader_handle());
    endpoint_manager.init_metrics();

    let state = setup_services(config, endpoint_manager);

    let mut tasks = JoinSet::new();
    let mut task_names = HashMap::new();

    {
        // API SERVER
        let server_shutdown = shutdown.child_token();
        let metrics_registry = metrics_state.registry.clone();
        let handle = tasks.spawn(async move {
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
        task_names.insert(handle.id(), "API server");
    }

    {
        // Kubernetes controllers
        let token = shutdown.child_token();
        let metrics_registry = metrics_state.registry.clone();
        let handle = tasks.spawn(async move {
            sarena_kubernetes::server::start(metrics_registry, token)
                .await
                .map_err(anyhow::Error::from)
        });
        task_names.insert(handle.id(), "Kubernetes controllers");
    }

    // Wait for either an OS shutdown signal or a task exiting unexpectedly
    // (e.g. panicking, or returning before we asked it to stop). Whichever
    // happens first triggers the same graceful shutdown path below.
    // `biased` so shutdown wins when both branches are ready: tasks return as
    // soon as their child token is cancelled, which must not count as an early exit.
    let early_exit = tokio::select! {
        biased;

        () = shutdown.cancelled() => {
            info!("shutdown signal received");
            false
        }

        Some(result) = tasks.join_next_with_id() => {
            error!("daemon task exited before shutdown was requested");
            log_task_result(&mut task_names, result);
            true
        }
    };

    // No-op on the signal path; on the early-exit path this tells the
    // remaining tasks to stop gracefully.
    shutdown.cancel();

    let deadline = Instant::now() + SHUTDOWN_BUDGET;

    shutdown_step("daemon tasks", SHUTDOWN_TIMEOUT, deadline, async {
        while let Some(result) = tasks.join_next_with_id().await {
            log_task_result(&mut task_names, result);
        }
    })
    .await;

    if !tasks.is_empty() {
        let remaining: Vec<_> = task_names.values().collect();
        warn!(
            ?remaining,
            "aborting daemon tasks that did not stop in time"
        );
        tasks.abort_all();
    }

    shutdown_step("data plane", SHUTDOWN_TIMEOUT, deadline, async {
        if let Err(err) = data_plane.shutdown().await {
            error!(?err, "failed to shutdown data plane");
        }
    })
    .await;

    shutdown_step(
        "OTel meter provider",
        TELEMETRY_SHUTDOWN_TIMEOUT,
        deadline,
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
        deadline,
        shutdown_blocking("OTel tracer provider", logging::shutdown_tracing),
    )
    .await;

    if early_exit {
        anyhow::bail!("daemon task exited before shutdown was requested");
    }

    Ok(())
}

fn setup_services(config: DataPlaneConfig, endpoint_manager: EndpointManager) -> AppState {
    let ipam = Arc::new(DefaultIpamService::new(
        config.gateway_ip,
        config.ipam_ipv4_subnet,
        config.ipam_ipv6_subnet,
    ));
    let endpoint: Arc<DefaultEndpointService> =
        Arc::new(DefaultEndpointService::new(endpoint_manager));
    let daemon = Arc::new(DefaultDaemonService::new());

    AppState::new(ipam, endpoint, daemon)
}

/// Runs one shutdown step, bounded by its own `timeout` and by the overall
/// shutdown `deadline`, whichever comes first.
async fn shutdown_step(
    name: &str,
    timeout: Duration,
    deadline: Instant,
    fut: impl Future<Output = ()>,
) {
    let step_deadline = deadline.min(Instant::now() + timeout);
    if tokio::time::timeout_at(step_deadline, fut).await.is_err() {
        error!("timed out shutting down {name}");
    }
}

async fn shutdown_blocking(name: &str, f: impl FnOnce() + Send + 'static) {
    if tokio::task::spawn_blocking(f).await.is_err() {
        error!("{name} shutdown task panicked");
    }
}

/// Logs a finished task under its name, removing it from `names` so that what
/// is left there are the tasks still running.
fn log_task_result(
    names: &mut HashMap<Id, &'static str>,
    result: Result<(Id, anyhow::Result<()>), JoinError>,
) {
    let id = match &result {
        Ok((id, _)) => *id,
        Err(err) => err.id(),
    };
    let task = names.remove(&id).unwrap_or("unknown");
    match result {
        Ok((_, Ok(()))) => info!(task, "daemon task exited"),
        Ok((_, Err(err))) => error!(task, ?err, "daemon task failed"),
        Err(err) => error!(task, ?err, "daemon task panicked"),
    }
}
