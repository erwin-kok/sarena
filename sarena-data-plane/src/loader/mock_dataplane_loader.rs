use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;

use crate::{
    error::{DataPlaneError, Res},
    loader::{DataplaneLoader, LoaderEndpoint, LoaderEndpointKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderCall {
    LoadGlobalMaps,
    AddLoaderEndpoint(LoaderEndpointKind, String),
    RemoveLoaderEndpoint(LoaderEndpointKind, String),
    ListLoaderEndpoints,
    Reconcile(Vec<(LoaderEndpointKind, String)>),
    TeardownAll,
    Shutdown,
}

/// In-memory [`DataplaneLoader`]: records every call and keeps the set of
/// active endpoints, without touching bpf(2) or netlink.
#[derive(Debug, Default)]
pub struct MockDataplaneLoader {
    state: Mutex<MockState>,
}

#[derive(Debug, Default)]
struct MockState {
    calls: Vec<LoaderCall>,
    endpoints: Vec<(LoaderEndpointKind, String)>,
    fail_next: Option<String>,
    shut_down: bool,
}

impl MockDataplaneLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn calls(&self) -> Vec<LoaderCall> {
        self.lock().calls.clone()
    }

    pub fn endpoints(&self) -> Vec<(LoaderEndpointKind, String)> {
        self.lock().endpoints.clone()
    }

    /// Makes the next call fail with [`DataPlaneError::Injected`].
    pub fn fail_next(&self, msg: impl Into<String>) {
        self.lock().fail_next = Some(msg.into());
    }

    fn lock(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records `call` and returns the guard, or the injected/shutdown error.
    fn record(&self, call: LoaderCall) -> Res<MutexGuard<'_, MockState>> {
        let mut state = self.lock();
        state.calls.push(call);
        if state.shut_down {
            return Err(DataPlaneError::ActorGone);
        }
        if let Some(msg) = state.fail_next.take() {
            return Err(DataPlaneError::Injected(msg));
        }
        Ok(state)
    }
}

#[async_trait]
impl DataplaneLoader for MockDataplaneLoader {
    async fn load_global_maps(&self) -> Res<()> {
        drop(self.record(LoaderCall::LoadGlobalMaps)?);
        Ok(())
    }

    async fn add_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()> {
        let (kind, link) = (endpoint.kind(), endpoint.link());
        let mut state = self.record(LoaderCall::AddLoaderEndpoint(kind, link.to_string()))?;
        let endpoint = (kind, link.to_string());
        if !state.endpoints.contains(&endpoint) {
            state.endpoints.push(endpoint);
        }
        Ok(())
    }

    async fn remove_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()> {
        let (kind, link) = (endpoint.kind(), endpoint.link());
        let mut state = self.record(LoaderCall::RemoveLoaderEndpoint(kind, link.to_string()))?;
        state.endpoints.retain(|(k, l)| !(*k == kind && l == link));
        Ok(())
    }

    async fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>> {
        let state = self.record(LoaderCall::ListLoaderEndpoints)?;
        Ok(state.endpoints.clone())
    }

    async fn reconcile(
        &self,
        desired: Vec<LoaderEndpoint>,
    ) -> Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>> {
        let desired: Vec<_> = desired
            .iter()
            .map(|endpoint| (endpoint.kind(), endpoint.link().to_string()))
            .collect();
        let mut state = self.record(LoaderCall::Reconcile(desired.clone()))?;
        state.endpoints.clear();
        for endpoint in desired {
            if !state.endpoints.contains(&endpoint) {
                state.endpoints.push(endpoint);
            }
        }
        Ok(Vec::new())
    }

    async fn teardown_all(&self) -> Res<()> {
        drop(self.record(LoaderCall::TeardownAll)?);
        Ok(())
    }

    async fn shutdown(&self) -> Res<()> {
        let mut state = self.record(LoaderCall::Shutdown)?;
        state.shut_down = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_tracks_endpoints_and_records_calls() {
        let loader = MockDataplaneLoader::new();
        let kind = LoaderEndpointKind::Host;
        let host = |link: &str| LoaderEndpoint::Host {
            link: link.to_string(),
        };

        loader.add_loader_endpoint(&host("lxc0")).await.unwrap();
        loader.add_loader_endpoint(&host("lxc1")).await.unwrap();
        loader.remove_loader_endpoint(&host("lxc0")).await.unwrap();

        assert_eq!(
            loader.list_loader_endpoints().await.unwrap(),
            vec![(kind, "lxc1".to_string())]
        );
        assert_eq!(
            loader.calls(),
            vec![
                LoaderCall::AddLoaderEndpoint(kind, "lxc0".to_string()),
                LoaderCall::AddLoaderEndpoint(kind, "lxc1".to_string()),
                LoaderCall::RemoveLoaderEndpoint(kind, "lxc0".to_string()),
                LoaderCall::ListLoaderEndpoints,
            ]
        );
    }

    #[tokio::test]
    async fn mock_fails_injected_call_and_after_shutdown() {
        let loader = MockDataplaneLoader::new();

        loader.fail_next("boom");
        assert!(matches!(
            loader.load_global_maps().await,
            Err(DataPlaneError::Injected(_))
        ));
        loader.load_global_maps().await.unwrap();

        loader.shutdown().await.unwrap();
        assert!(matches!(
            loader.teardown_all().await,
            Err(DataPlaneError::ActorGone)
        ));
    }
}
