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

use std::{collections::HashSet, net::IpAddr};

pub use actor::DefaultDataplaneLoader;
use async_trait::async_trait;
pub use aya_backend::AyaBackend;
pub use data_plane_loader::Loader;
pub use loader_endpoint::{LoaderEndpoint, LoaderEndpointKind};
#[cfg(any(test, feature = "test-util"))]
pub use mock_dataplane_loader::{LoaderCall, MockDataplaneLoader};
pub use pin::PinRoot;

use crate::error::Res;

#[async_trait]
pub trait DataplaneLoader: Send + Sync {
    async fn load_global_maps(&self) -> Res<()>;

    /// Loads and attaches `endpoint`'s programs and fills in its maps.
    async fn add_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()>;

    /// Removes `endpoint`'s map entries, detaches its programs and unpins
    /// its per-endpoint maps.
    async fn remove_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()>;

    async fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>>;

    /// Whether network interface `link` exists. `Ok(false)` only when it is
    /// known not to; a failed lookup is an error.
    async fn link_exists(&self, link: &str) -> Res<bool>;

    /// Detaches the programs of the endpoint known only by `kind` and `link`
    /// and unpins its maps, e.g. a stale one found on restart. Its `lxc_map`
    /// entries are left alone, see [`Self::prune_endpoint_infos`].
    async fn detach_loader_endpoint(&self, kind: LoaderEndpointKind, link: &str) -> Res<()>;

    /// Drops every `lxc_map` entry whose IP is not in `keep`, except the
    /// host's.
    async fn prune_endpoint_infos(&self, keep: HashSet<IpAddr>) -> Res<()>;

    async fn teardown_all(&self) -> Res<()>;

    /// Tears everything down and stops the loader. Any call made after
    /// this (from any clone) fails.
    async fn shutdown(&self) -> Res<()>;
}
