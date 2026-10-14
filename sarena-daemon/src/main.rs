use std::{
    env::consts,
    fs,
    future::Future,
    io::IsTerminal as _,
    net::{IpAddr, Ipv4Addr},
    process,
    time::Duration,
};

use clap::{CommandFactory as _, Parser as _};
use clap_markdown_generator::{
    CommandHeadingStyle, MarkdownOptions, ParameterContentStyle, ParameterHeadingStyle,
    generate_markdown_for_command_with_options,
};
use ipnet::{IpNet, Ipv4Net};
use sarena_api_types_v1::daemon::SarenaVersion;
use sarena_data_plane::{DataPlane as _, DataPlaneConfig, DefaultDataPlane, EndpointManager};
use sarena_services::setup_services;
use sarena_utils::{
    TracingConfig, logging, metrics::init_metrics, root::require_root_privilege, version,
};
use tokio::{
    signal::unix::{SignalKind, signal},
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::cli::Cli;

mod cli;
mod config;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const TELEMETRY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const TCP_PORT: u16 = 3000;

const BANNER: [&str; 5] = [
    r" ____    _    ____  _____ _   _    _    ",
    r"/ ___|  / \  |  _ \| ____| \ | |  / \   ",
    r"\___ \ / _ \ | |_) |  _| |  \| | / _ \  ",
    r" ___) / ___ \|  _ <| |___| |\  |/ ___ \ ",
    r"|____/_/   \_\_| \_\_____|_| \_|_/   \_\",
];

// 256-color codes, blue to purple
const SHADES: [u8; 5] = [33, 69, 105, 141, 177];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let command = Cli::command();
    if let Some(cmd_ref) = cli.cmd_ref {
        let markdown = generate_markdown_for_command_with_options(
            command,
            MarkdownOptions {
                include_subcommands: true,
                include_toc: true,
                command_heading: CommandHeadingStyle::Display,
                parameter_heading: ParameterHeadingStyle::Name,
                parameter_content: ParameterContentStyle::Text,
                ..MarkdownOptions::default()
            },
        );
        fs::write(cmd_ref, markdown)?;
        process::exit(0)
    }

    require_root_privilege(command.get_name());

    init_daemon(cli).await?;

    process::exit(0)
}

async fn init_daemon(_cli: Cli) -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = config::load_config(&cli)?;

    logging::init_tracing(&TracingConfig {
        enable_debug: config.debug,
        log_file: config.log_file.clone(),
        format: config.log_format,
        ..Default::default()
    });

    print_banner();

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
    let state = setup_services(config, data_plane.loader_handle());

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

    let _endpoint_manager = EndpointManager::new();

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

    shutdown_step("data plane", SHUTDOWN_TIMEOUT, async {
        if let Err(err) = data_plane.shutdown().await {
            error!(?err, "failed to shutdown data plane");
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

    Ok(())
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
