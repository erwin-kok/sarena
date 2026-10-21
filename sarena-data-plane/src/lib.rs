mod config;
mod controller;
mod endpoint;
mod error;
mod loader;
mod maps;
mod models;
mod netlink;
mod setup;
mod soft_dataplane;

use std::sync::Arc;

use async_trait::async_trait;
pub use config::DataPlaneConfig;
pub use controller::{
    Backoff, BoxError, ControllerObserver, ControllerStatus, Flow, JobFn, Manager, ManagerBuilder,
    Registry, Spec, StopFn, Termination, job, stop_fn,
};
pub use endpoint::{EndpointId, EndpointManager, EndpointPersist, EndpointSpec};
pub use error::{DataPlaneError, Res};
pub use loader::{
    AyaBackend, DataplaneLoader, DefaultDataplaneLoader, Loader, LoaderEndpointKind, PinRoot,
};
#[cfg(any(test, feature = "test-util"))]
pub use loader::{LoaderCall, MockDataplaneLoader};
pub use maps::{CallsMap, EndpointConfigMap, LxcMap, MetricsMap};
pub use sarena_infra::{InterfaceAddress, MacAddress};
pub use setup::DefaultDataPlane;

/// Where eBPF program/link state gets pinned.
pub const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

#[async_trait]
pub trait DataPlane: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn shutdown(self) -> Res<()>;

    fn loader(&self) -> Arc<dyn DataplaneLoader>;
}
