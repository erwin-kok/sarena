use std::fs;

use anyhow::Result;
use clap::CommandFactory as _;
use clap_markdown_generator::{
    CommandHeadingStyle, MarkdownOptions, ParameterContentStyle, ParameterHeadingStyle,
    generate_markdown_for_command_with_options,
};
use sarena_api_client::{ApiClient, TransportKind};

use crate::{
    cli::{Cli, Commands, bpf, completion, service, version},
    config::Config,
};

pub struct App {
    pub config: Config,
    pub client: ApiClient<TransportKind>,
}

impl App {
    pub async fn run(&self, command: &Commands) -> Result<()> {
        match command {
            Commands::Bpf(command) => bpf::run(self, command)?,
            Commands::Completion(args) => completion::run(args),
            Commands::Service(command) => service::run(command),
            Commands::Version(args) => version::run(self, args.output.as_ref()).await?,
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
        Ok(())
    }
}
