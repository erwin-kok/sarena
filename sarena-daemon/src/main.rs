use std::{env::consts, fs, process};

use clap::{CommandFactory as _, Parser as _};
use clap_markdown_generator::{
    CommandHeadingStyle, MarkdownOptions, ParameterContentStyle, ParameterHeadingStyle,
    generate_markdown_for_command_with_options,
};
use sarena_api_types_v1::daemon::SarenaVersion;
use sarena_utils::{
    cli_output::{self, OutputFormat},
    root::require_root_privilege,
    version,
};

use crate::{
    cli::{Cli, Commands},
    init::init_daemon,
};

mod cli;
mod config;
mod init;
mod metrics;

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
