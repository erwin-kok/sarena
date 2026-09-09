use std::{path::PathBuf, str::FromStr};

use clap::{Args, Parser, Subcommand};

pub mod bpf;
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
    /// Path to a configuration file to load before applying the flags below.
    /// When omitted, `~/.sarena` is read if it exists. See docs/config.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Emit debug-level log messages (overrides `debug` from the config file).
    #[arg(short = 'D', long, global = true)]
    pub debug: bool,

    /// Address of the daemon to talk to, as `unix://<path>` or `tcp://<host:port>`.
    /// Overrides `host` from the config file.
    /// Default: `unix:///tmp/sarena.sock`.
    #[arg(short = 'H', long, global = true)]
    pub host: Option<String>,

    /// Write logs to this file instead of stderr (overrides `log_file` from the
    /// config file).
    #[arg(short = 'L', long, global = true)]
    pub log_file: Option<String>,

    /// Log output format: `text` (one line per event, for a terminal) or `json`
    /// (structured, for production / log aggregation).
    /// Overrides `log_format` from the config file. Default: `text`.
    #[arg(short = 'F', long, global = true, value_parser = ["text", "json"])]
    pub log_format: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Args, Debug)]
pub struct OutputArgs {
    /// Render the result as machine-readable output instead of a table:
    /// `json`, `yaml`, or `jsonpath=<expression>`.
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
    /// Direct access to local BPF maps
    Bpf(bpf::BpfCommand),

    /// Installing bash/zsh/fish completion
    Completion(completion::CompletionArgs),

    /// List services & loadbalancers
    Service(service::ServiceCommand),

    /// Print version, git commit, and build date/time
    Version(OutputArgs),
}
