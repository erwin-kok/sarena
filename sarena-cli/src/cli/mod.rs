use std::{path::PathBuf, str::FromStr};

use clap::{Args, Parser, Subcommand};

pub mod completion;
pub mod service;
pub mod version;

#[derive(Parser, Debug)]
#[command(
    name = "sarena-cli",
    about = "sarena-cli",
    long_about = "CLI for interacting with the local Sarena daemon"
)]
pub struct Cli {
    /// Config file
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Enable debug messages
    #[arg(short = 'D', long, global = true)]
    pub debug: bool,

    #[arg(short = 'H', long, global = true)]
    pub host: Option<String>,

    #[arg(short = 'L', long, global = true)]
    pub log_file: Option<String>,

    /// Log format: "text" (one-line, for a terminal) or "json" (for
    /// production/log aggregation)
    #[arg(short = 'F', long, global = true, value_parser = ["text", "json"])]
    pub log_format: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args, Debug)]
pub struct OutputArgs {
    #[arg(short = 'o', long)]
    pub output: Option<OutputFormat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Yaml,
    JsonPath(String),
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            value => {
                if let Some(expression) = value.strip_prefix("jsonpath=") {
                    Ok(Self::JsonPath(expression.to_string()))
                } else {
                    Err(format!(
                        "invalid output format: {value}; expected json, yaml, or jsonpath=<expression>"
                    ))
                }
            }
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Installing bash/zsh/fish completion
    Completion(completion::CompletionArgs),

    /// List services & loadbalancers
    Service(service::ServiceCommand),

    /// Print version, git commit, and build date/time
    Version(OutputArgs),
}
