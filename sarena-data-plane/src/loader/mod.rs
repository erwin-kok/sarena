mod actor;
mod aya_backend;
mod backend;
mod data_plane_loader;
mod loader_endpoint;
mod manifest;
mod pin;

#[cfg(test)]
mod mock_backend;
#[cfg(any(test, feature = "test-util"))]
mod mock_dataplane_loader;

pub use actor::DefaultDataplaneLoader;
use async_trait::async_trait;
pub use aya_backend::AyaBackend;
pub use data_plane_loader::Loader;
pub use loader_endpoint::{LoaderEndpoint, LoaderEndpointKind};
#[cfg(any(test, feature = "test-util"))]
pub use mock_dataplane_loader::{LoaderCall, MockDataplaneLoader};
pub use pin::PinRoot;

use crate::error::{DataPlaneError, Res};

#[async_trait]
pub trait DataplaneLoader: Send + Sync {
    async fn load_global_maps(&self) -> Res<()>;

    /// Loads and attaches `endpoint`'s programs and fills in its maps.
    async fn add_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()>;

    /// Removes `endpoint`'s map entries, detaches its programs and unpins
    /// its per-endpoint maps.
    async fn remove_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()>;

    async fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>>;

    /// Brings the active endpoints and the `lxc_map` in line with
    /// `desired`, which must be the complete set. Per-endpoint failures are
    /// returned rather than aborting the whole reconcile.
    async fn reconcile(
        &self,
        desired: Vec<LoaderEndpoint>,
    ) -> Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>>;

    async fn teardown_all(&self) -> Res<()>;

    /// Tears everything down and stops the loader. Any call made after
    /// this (from any clone) fails.
    async fn shutdown(&self) -> Res<()>;
}
