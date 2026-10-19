use std::{env::consts, fs, io::IsTerminal as _, process};

use anyhow::Context as _;
use clap::{CommandFactory as _, Parser as _};
use clap_markdown_generator::{
    CommandHeadingStyle, MarkdownOptions, ParameterContentStyle, ParameterHeadingStyle,
    generate_markdown_for_command_with_options,
};
use sarena_api_types_v1::daemon::SarenaVersion;
use sarena_utils::{
    TracingConfig,
    cli_output::{self, OutputFormat},
    logging,
    root::require_root_privilege,
    version,
};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::cli::{Cli, Commands};

mod cli;
mod config;
mod init;

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
    let mut cli = Cli::parse();
    let command = cli.command.take().unwrap_or(Commands::Start);
    match command {
        Commands::Start => {
            let command = Cli::command();
            require_root_privilege(command.get_name());
            if let Err(err) = init_daemon(cli).await {
                // Tracing may already be shut down at this point, so print directly.
                eprintln!("Error: {err:?}");
                process::exit(1);
            }
        }
        Commands::Version(args) => version(args.output.as_ref()),
        Commands::CmdRef { path } => {
            let command = Cli::command();
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
            fs::write(path, markdown)?;
        }
    }

    process::exit(0)
}

fn version(format: Option<&OutputFormat>) {
    let client_version = SarenaVersion {
        version: version::VERSION.to_owned(),
        git_hash: version::GIT_HASH.to_owned(),
        build_date: version::BUILD_DATE.to_owned(),
        os: consts::OS.to_owned(),
        arch: consts::ARCH.to_owned(),
    };
    if let Some(f) = format {
        let _ = cli_output::print_output(&client_version, f);
    } else {
        println!("client version: {client_version}");
    }
}

async fn init_daemon(cli: Cli) -> anyhow::Result<()> {
    let config = config::load_config(&cli)?;

    logging::init_tracing(&TracingConfig {
        enable_debug: config.debug,
        log_file: config.log_file.clone(),
        format: config.log_format,
        ..Default::default()
    });

    print_banner();

    let shutdown = CancellationToken::new();
    spawn_signal_handler(shutdown.clone())?;

    init::run_daemon(&config, &shutdown).await
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
