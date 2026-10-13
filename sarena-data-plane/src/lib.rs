mod asyncprim;
mod config;
mod container;
mod endpoint;
mod endpoint_events;
mod endpoint_manager;
mod error;
mod id_allocator;
mod loader;
mod maps;
mod models;
mod netlink;
mod reconciler;
mod setup;
mod soft_dataplane;

use async_trait::async_trait;
pub use asyncprim::*;
pub use config::DataPlaneConfig;
pub use container::add_container;
pub use error::{DataPlaneError, Res};
pub use loader::{AyaBackend, Loader, LoaderEndpointKind, LoaderHandle, PinRoot};
pub use maps::{CallsMap, EndpointConfigMap, LxcMap, MetricsMap};
pub use sarena_infra::{InterfaceAddress, MacAddress};
pub use setup::DefaultDataPlane;

/// Where eBPF program/link state gets pinned.
pub const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

#[async_trait]
pub trait DataPlane: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn shutdown(self) -> Res<()>;

    fn loader_handle(&self) -> LoaderHandle;
}
