mod config;
mod error;
mod loader;
mod maps;
mod models;
mod netlink;
mod reconciler;
mod setup;
mod soft_dataplane;

pub use config::DataPlaneConfig;
pub use error::DataPlaneError;
pub use loader::{AyaBackend, EndpointKind, Loader, LoaderHandle, PinRoot};
pub use maps::{CallsMap, EndpointConfigMap, LxcMap, MetricsMap};
pub use setup::start_control_plane;

/// Where eBPF program/link state gets pinned.
pub const PIN_ROOT: &str = "/sys/fs/bpf/sarena";
