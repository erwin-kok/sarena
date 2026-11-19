use std::{
    env::consts,
    fs,
    io::IsTerminal as _,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
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
use sarena_infra::{NetlinkNetworkProvisioner, NetworkProvisioner};
use sarena_kubernetes::client::ClientOptions;
use sarena_services::{AppState, DefaultDaemonService, DefaultEndpointService, DefaultIpamService};
use sarena_utils::{TracingConfig, logging, version};
use semver::{Version, VersionReq};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    cli::Cli,
    config::{Config, load_config},
    metrics::BootstrapStatistics,
    node::LocalNode,
    shutdown::{DaemonTasks, spawn_signal_handler, wait_for_shutdown},
};

const MINIMUM_KERNEL_VESRION: &str = "5.10.0";
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

    let mut network_provisioner = NetlinkNetworkProvisioner;

    let mut local_node = LocalNode::default();
    local_node
        .initialize(&network_provisioner, &config, None)
        .await?;

    if let Some(ipv4) = config.ipv4_node_address {
        local_node.ipv4_external_address = Some(ipv4);
    }

    if let Some(ipv6) = config.ipv6_node_address {
        local_node.ipv6_address = Some(ipv6);
    }

    network_provisioner
        .enable_ip_forwarding(config.enable_ipv6)
        .await?;

    // Connecting may retry for a while; don't let that hold up a shutdown.
    let k8s_client = tokio::select! {
        () = shutdown.cancelled() => {
            info!("shutdown requested while connecting to Kubernetes");
            return Ok(());
        }
        client = connect_kubernetes(&config, &bootstrap_statistics) => client?,
    };

    //

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

    let mut tasks = DaemonTasks::new();
    spawn_metrics_upkeep(&mut tasks, &shutdown, prometheus_handle.clone());
    spawn_api_server(
        &mut tasks,
        &shutdown,
        config.socket_path.clone(),
        state,
        prometheus_handle,
    );
    if let Some(k8s_client) = k8s_client {
        spawn_kubernetes(
            &mut tasks,
            &shutdown,
            k8s_client,
            config.k8s_heartbeat_timeout,
        );
    }

    bootstrap_statistics.overall.end(true);
    bootstrap_statistics.update_metrics();

    wait_for_shutdown(tasks, shutdown, data_plane).await
}

/// Metrics upkeep: `install_recorder` (unlike `install`) does not spawn this
/// itself.
fn spawn_metrics_upkeep(
    tasks: &mut DaemonTasks,
    shutdown: &CancellationToken,
    prometheus_handle: PrometheusHandle,
) {
    let token = shutdown.child_token();
    tasks.spawn("metrics upkeep", async move {
        run_metrics_upkeep(prometheus_handle, token).await;
        Ok(())
    });
}

/// The API server, on `socket_path` and on [`TCP_PORT`].
fn spawn_api_server(
    tasks: &mut DaemonTasks,
    shutdown: &CancellationToken,
    socket_path: PathBuf,
    state: AppState,
    prometheus_handle: PrometheusHandle,
) {
    let token = shutdown.child_token();
    tasks.spawn("API server", async move {
        sarena_api_server::ApiServer::new()
            .start(&socket_path, TCP_PORT, state, prometheus_handle, token)
            .await
    });
}

/// Connects to the Kubernetes API server per `config`, waiting until it
/// answers (see [`sarena_kubernetes::client::connect`]). `None` when there
/// is no Kubernetes to connect to (see
/// [`sarena_kubernetes::client::is_configured`]), in which case the daemon
/// runs without its Kubernetes controllers.
async fn connect_kubernetes(
    config: &Config,
    bootstrap_statistics: &BootstrapStatistics,
) -> anyhow::Result<Option<kube::Client>> {
    let options = ClientOptions {
        api_server_url: config.k8s_api_server_url.clone(),
        kubeconfig_path: config.k8s_kubeconfig_path.clone(),
        heartbeat_timeout: config.k8s_heartbeat_timeout,
    };
    if !sarena_kubernetes::client::is_configured(&options) {
        info!("no Kubernetes configuration found, running without Kubernetes");
        return Ok(None);
    }

    bootstrap_statistics.k8s_init.start();
    let result = sarena_kubernetes::client::connect(&options).await;
    bootstrap_statistics.k8s_init.end(result.is_ok());

    result
        .map(Some)
        .context("unable to create Kubernetes client")
}

/// The Kubernetes controllers, plus the API server heartbeat when
/// `heartbeat_timeout` is set.
fn spawn_kubernetes(
    tasks: &mut DaemonTasks,
    shutdown: &CancellationToken,
    client: kube::Client,
    heartbeat_timeout: Option<Duration>,
) {
    if let Some(timeout) = heartbeat_timeout {
        spawn_kubernetes_heartbeat(tasks, shutdown, client.clone(), timeout);
    }
    spawn_kubernetes_controllers(tasks, shutdown, client);
}

/// Periodically checks the API server's health, see
/// [`sarena_kubernetes::client::run_heartbeat`].
fn spawn_kubernetes_heartbeat(
    tasks: &mut DaemonTasks,
    shutdown: &CancellationToken,
    client: kube::Client,
    timeout: Duration,
) {
    let token = shutdown.child_token();
    tasks.spawn("Kubernetes heartbeat", async move {
        sarena_kubernetes::client::run_heartbeat(client, timeout, token).await;
        Ok(())
    });
}

fn spawn_kubernetes_controllers(
    tasks: &mut DaemonTasks,
    shutdown: &CancellationToken,
    client: kube::Client,
) {
    let token = shutdown.child_token();
    tasks.spawn("Kubernetes controllers", async move {
        sarena_kubernetes::server::start(client, token)
            .await
            .map_err(anyhow::Error::from)
    });
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
