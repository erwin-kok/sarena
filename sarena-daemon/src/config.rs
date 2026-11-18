use std::{env::home_dir, net::Ipv4Addr, path::PathBuf};

use config::{Config as ConfigLoader, Environment, File};
use sarena_utils::LogFormat;
use serde::Deserialize;

use crate::cli::Cli;

pub const DEFAULT_RUNTIME_PATH: &str = "/var/run/sarena";

/// Effective daemon configuration, merged from (lowest to highest precedence):
/// the config file (`--config <path>`, or `~/.sarena/sarena-daemon.{toml,json,yaml,yml}`
/// when it exists), `SARENA_<FIELD>` environment variables (e.g. `SARENA_SOCKET_PATH`),
/// and finally the command-line flags.
#[derive(Debug, Clone)]
pub struct Config {
    /// Emit debug-level log messages.
    pub debug: bool,

    /// Write logs to this file instead of stderr.
    pub log_file: Option<String>,

    /// Log output format: `text` (default) or `json`.
    pub log_format: LogFormat,

    /// BPF filesystem directory under which the datapath's maps and programs
    /// are pinned. Default: `/sys/fs/bpf/sarena`.
    pub pin_root: PathBuf,

    /// Base directory for the daemon's runtime files. Used to derive the
    /// defaults of `statedir_path` and `socket_path`. Default: `/var/run/sarena`.
    pub runtime_path: PathBuf,

    /// Directory where per-endpoint state is persisted, so endpoints can be
    /// restored after a daemon restart. Default: `<runtime_path>/state`.
    pub statedir_path: PathBuf,

    /// Unix domain socket the API server listens on (the CNI plugin and CLI
    /// connect to it). Default: `<runtime_path>/sarena.sock`.
    pub socket_path: PathBuf,

    /// IPv4 address of the node-side gateway that endpoints use as their
    /// default route.
    pub gateway_ip: Option<Ipv4Addr>,

    /// IPv4 address of the node on the internal (pod-facing) network.
    pub internal_ip: Option<Ipv4Addr>,

    /// IPv4 subnet in CIDR notation (e.g. `10.0.10.0/24`) from which IPAM
    /// allocates endpoint addresses.
    pub ipam_ipv4_subnet: Option<String>,
}

#[derive(Deserialize)]
struct ConfigBuilder {
    #[serde(default)]
    debug: bool,

    #[serde(default)]
    log_file: Option<String>,

    #[serde(default)]
    log_format: LogFormat,

    #[serde(default = "default_pin_root")]
    pin_root: PathBuf,

    #[serde(default = "default_runtime_path")]
    runtime_path: PathBuf,

    statedir_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,

    gateway_ip: Option<Ipv4Addr>,
    internal_ip: Option<Ipv4Addr>,
    ipam_ipv4_subnet: Option<String>,
}

fn default_pin_root() -> PathBuf {
    PathBuf::from(sarena_data_plane::PIN_ROOT)
}

fn default_runtime_path() -> PathBuf {
    PathBuf::from(DEFAULT_RUNTIME_PATH)
}

impl From<ConfigBuilder> for Config {
    fn from(config: ConfigBuilder) -> Self {
        let statedir_path = config
            .statedir_path
            .unwrap_or_else(|| config.runtime_path.join("state"));
        let socket_path = config
            .socket_path
            .unwrap_or_else(|| config.runtime_path.join("sarena.sock"));
        Self {
            debug: config.debug,
            log_file: config.log_file,
            log_format: config.log_format,
            pin_root: config.pin_root,
            runtime_path: config.runtime_path,
            statedir_path,
            socket_path,
            gateway_ip: config.gateway_ip,
            internal_ip: config.internal_ip,
            ipam_ipv4_subnet: config.ipam_ipv4_subnet,
        }
    }
}

pub fn load_config(cli: &Cli) -> anyhow::Result<Config> {
    let mut builder = ConfigLoader::builder();

    if let Some(path) = &cli.config {
        builder = builder.add_source(File::from(path.clone()));
    } else if let Some(home) = home_dir() {
        let path = ["toml", "json", "yaml", "yml"]
            .iter()
            .map(|ext| home.join(".sarena").join(format!("sarena-daemon.{ext}")))
            .find(|path| path.is_file());
        if let Some(path) = path {
            builder = builder.add_source(File::from(path));
        }
    }

    builder = builder.add_source(Environment::with_prefix("SARENA"));

    let config: ConfigBuilder = builder.build()?.try_deserialize()?;
    let mut config = Config::from(config);

    if cli.debug {
        config.debug = true;
    }

    if cli.log_file.is_some() {
        config.log_file.clone_from(&cli.log_file);
    }

    if let Some(log_format) = &cli.log_format {
        config.log_format = log_format.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    }

    Ok(config)
}
