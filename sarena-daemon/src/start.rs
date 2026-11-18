use std::{
    collections::HashMap,
    env::consts,
    fs,
    future::Future,
    io::IsTerminal as _,
    net::{IpAddr, Ipv4Addr},
    process,
    sync::Arc,
    time::Duration,
};

use anyhow::Context as _;
use ipnet::{IpNet, Ipv4Net};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use sarena_api_types_v1::daemon::SarenaVersion;
use sarena_data_plane::{
    DataPlane as _, DataPlaneConfig, DefaultDataPlane, EndpointManager, PinRoot,
};
use sarena_infra::NetlinkNetworkProvisioner;
use sarena_services::{AppState, DefaultDaemonService, DefaultEndpointService, DefaultIpamService};
use sarena_utils::{TracingConfig, logging, version};
use semver::{Version, VersionReq};
use tokio::{
    signal::unix::{SignalKind, signal},
    task::{Id, JoinError, JoinSet},
    time::{Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{cli::Cli, config::load_config, metrics::BootstrapStatistics, node::LocalNode};

const MINIMUM_KERNEL_VESRION: &str = "5.10.0";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const TELEMETRY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(25);
const TCP_PORT: u16 = 3000;
/// Same interval `PrometheusBuilder::install` would use for its own upkeep task.
const METRICS_UPKEEP_INTERVAL: Duration = Duration::from_secs(5);

const BANNER: [&str; 5] = [
    r" ____    _    ____  _____ _   _    _    ",
    r"/ ___|  / \  |  _ \| ____| \ | |  / \   ",
    r"\___ \ / _ \ | |_) |  _| |  \| | / _ \  ",
    r" ___) / ___ \|  _ <| |___| |\  |/ ___ \ ",
    r"|____/_/   \_\_| \_\_____|_| \_|_/   \_\",
];

// 256-color codes, blue to purple
const SHADES: [u8; 5] = [33, 69, 105, 141, 177];

pub async fn start_daemon(cli: Cli) -> anyhow::Result<()> {
    let config = load_config(&cli)?;

    logging::init_tracing(&TracingConfig {
        enable_debug: config.debug,
        log_file: config.log_file.clone(),
        format: config.log_format,
    });

    print_banner();

    let version = get_kernel_version()?;
    info!("Linux kernel version: {version}");
    check_kernel_version(&version, MINIMUM_KERNEL_VESRION)?;

    let bootstrap_statistics = BootstrapStatistics::new();
    bootstrap_statistics.overall.start();

    let prometheus_handle = PrometheusBuilder::new()
        .with_recommended_naming(true)
        .install_recorder()?;

    let shutdown: CancellationToken = CancellationToken::new();
    spawn_signal_handler(shutdown.clone())?;

    fs::create_dir_all(&config.runtime_path).with_context(|| {
        format!(
            "failed to create runtime path directory {}",
            config.runtime_path.display()
        )
    })?;
    fs::create_dir_all(&config.statedir_path).with_context(|| {
        format!(
            "failed to create statedir directory {}",
            config.statedir_path.display()
        )
    })?;
    if let Some(parent) = config.socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }

    let pin_root = PinRoot::new(&config.pin_root);
    pin_root
        .ensure_bpffs()
        .context("BPF filesystem is not usable")?;

    if !config.enable_ipv4 && !config.enable_ipv6 {
        anyhow::bail!("IPv4 and/or IPv6 addressing must be enabled");
    }

    let network_provisioner = NetlinkNetworkProvisioner;

    let mut local_node = LocalNode::default();
    local_node
        .initialize(&network_provisioner, &config, None)
        .await?;

    info!("LocalNode config: {local_node:?}");

    let data_plane_config = DataPlaneConfig {
        gateway_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
        internal_ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)),
        ipam_ipv4_subnet: Some(IpNet::V4(Ipv4Net::new(Ipv4Addr::new(10, 0, 10, 0), 24)?)),
        ipam_ipv6_subnet: None,
        pin_root: pin_root.clone(),
    };

    let data_plane = DefaultDataPlane::new(&data_plane_config).await?;

    let endpoint_manager = EndpointManager::new(data_plane.loader(), &config.statedir_path);
    endpoint_manager.init_metrics();

    let (restored, to_clean) = endpoint_manager.restore_old_endpoints().await?;
    info!(
        "endpoints restored: {}, endpoints to clean: {}",
        restored.len(),
        to_clean.len()
    );

    let state = setup_services(data_plane_config, endpoint_manager);

    let mut tasks = JoinSet::new();
    let mut task_names = HashMap::new();

    {
        // Metrics upkeep: `install_recorder` (unlike `install`) does not spawn this itself.
        let token = shutdown.child_token();
        let prometheus_handle = prometheus_handle.clone();
        let handle = tasks.spawn(async move {
            run_metrics_upkeep(prometheus_handle, token).await;
            Ok(())
        });
        task_names.insert(handle.id(), "metrics upkeep");
    }

    {
        // API SERVER
        let server_shutdown = shutdown.child_token();
        let handle = tasks.spawn(async move {
            sarena_api_server::ApiServer::new()
                .start(
                    &config.socket_path,
                    TCP_PORT,
                    state,
                    prometheus_handle,
                    server_shutdown,
                )
                .await
        });
        task_names.insert(handle.id(), "API server");
    }

    {
        // Kubernetes controllers
        let token = shutdown.child_token();
        let handle = tasks.spawn(async move {
            sarena_kubernetes::server::start(token)
                .await
                .map_err(anyhow::Error::from)
        });
        task_names.insert(handle.id(), "Kubernetes controllers");
    }

    bootstrap_statistics.overall.end(true);
    bootstrap_statistics.update_metrics();

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

async fn run_metrics_upkeep(prometheus_handle: PrometheusHandle, shutdown: CancellationToken) {
    let mut interval = tokio::time::interval(METRICS_UPKEEP_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = interval.tick() => prometheus_handle.run_upkeep(),
        }
    }
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

fn spawn_signal_handler(shutdown: CancellationToken) -> anyhow::Result<()> {
    let mut sigint = signal(SignalKind::interrupt()).context("failed to install SIGINT handler")?;
    let mut sigterm =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigint.recv() => info!("received SIGINT, shutting down"),
            _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
        }
        shutdown.cancel();

        let kind = tokio::select! {
            _ = sigint.recv() => SignalKind::interrupt(),
            _ = sigterm.recv() => SignalKind::terminate(),
        };
        error!("received second signal, forcing exit");
        process::exit(128 + kind.as_raw_value());
    });

    Ok(())
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

pub fn print_banner() {
    let color = std::io::stderr().is_terminal();
    for (line, shade) in BANNER.iter().zip(SHADES) {
        if color {
            eprintln!("\x1b[1;38;5;{shade}m{line}\x1b[0m");
        } else {
            eprintln!("{line}");
        }
    }
    let version = SarenaVersion {
        version: version::VERSION.to_owned(),
        git_hash: version::GIT_HASH.to_owned(),
        build_date: version::BUILD_DATE.to_owned(),
        os: consts::OS.to_owned(),
        arch: consts::ARCH.to_owned(),
    };
    if color {
        eprintln!("\x1b[38;5;214mSarena {version}\x1b[0m");
    } else {
        eprintln!("Sarena {version}");
    }
}

fn get_kernel_version() -> anyhow::Result<Version> {
    let release = fs::read_to_string("/proc/sys/kernel/osrelease")?;
    parse_kernel_version(release.trim())
}

fn check_kernel_version(kernel_version: &Version, min_version: &str) -> anyhow::Result<()> {
    let requirement = VersionReq::parse(&format!(">={min_version}"))?;
    if !requirement.matches(kernel_version) {
        anyhow::bail!(
            "kernel version {kernel_version} does not satisfy minimum version {min_version}"
        );
    }
    Ok(())
}

fn parse_kernel_version(ver: &str) -> anyhow::Result<Version> {
    let parts: Vec<&str> = ver.split('.').collect();

    if parts.len() < 2 {
        anyhow::bail!("unable to get kernel version from {ver:?}");
    }

    let major = parts[0].parse().context("invalid kernel major version")?;

    let minor = parts[1].parse().context("invalid kernel minor version")?;

    let patch = if parts.len() < 3 {
        0
    } else {
        let digits: String = parts[2].chars().take_while(char::is_ascii_digit).collect();

        if digits.is_empty() {
            0
        } else {
            digits.parse().context("invalid kernel patch version")?
        }
    };

    Ok(Version::new(major, minor, patch))
}
