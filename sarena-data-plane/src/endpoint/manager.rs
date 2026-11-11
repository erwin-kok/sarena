use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
    sync::{Arc, PoisonError, RwLock},
    time::Duration,
};

use tracing::{info, warn};

use super::{
    Endpoint, EndpointId,
    id_allocator::{IdAllocator, IdLease},
};
use crate::{
    DataPlaneError, DataplaneLoader, EndpointPersist, LoaderEndpoint, LoaderEndpointKind, Res,
    endpoint::{
        directory::{read_endpoints, remove_directories},
        id::EndpointIdentification,
    },
};

struct EndpointManagerInner {
    endpoints: HashMap<EndpointId, Endpoint>,
    container_name: HashMap<String, EndpointId>,
    container_id: HashMap<String, EndpointId>,
    docker_endpoint: HashMap<String, EndpointId>,
    ipv4: HashMap<Ipv4Addr, EndpointId>,
    ipv6: HashMap<Ipv6Addr, EndpointId>,
    pod_name: HashMap<String, EndpointId>,
}

impl EndpointManagerInner {
    pub fn new() -> Self {
        Self {
            endpoints: HashMap::new(),
            container_name: HashMap::new(),
            container_id: HashMap::new(),
            docker_endpoint: HashMap::new(),
            ipv4: HashMap::new(),
            ipv6: HashMap::new(),
            pod_name: HashMap::new(),
        }
    }

    pub fn add_endpoint(&mut self, ep: Endpoint) {
        self.endpoints.insert(ep.id(), ep);
    }

    pub fn remove_id(&mut self, id: EndpointId) -> Option<Endpoint> {
        self.endpoints.remove(&id)
    }

    pub fn remove_all(&mut self) {
        self.endpoints.clear();
        self.container_name.clear();
    }

    pub fn update_references(&mut self, id: EndpointId, endpoint: &EndpointPersist) {
        self.container_name.retain(|_, aux_id| *aux_id != id);
        if let Some(container_name) = &endpoint.container_name {
            self.container_name.insert(container_name.clone(), id);
        }
        if let Some(container_id) = &endpoint.container_id {
            self.container_id.insert(container_id.clone(), id);
        }
        if let Some(docker_endpoint) = &endpoint.container_id {
            self.docker_endpoint.insert(docker_endpoint.clone(), id);
        }
        if let Some(ipv4) = &endpoint.ipv4 {
            self.ipv4.insert(*ipv4, id);
        }
        if let Some(ipv6) = &endpoint.ipv6 {
            self.ipv6.insert(*ipv6, id);
        }
        if let Some(name) = endpoint.k8s_ns_and_pod_name() {
            self.pod_name.insert(name, id);
        }
    }

    pub fn remove_references(&mut self, id: EndpointId) {
        self.container_name.retain(|_, aux_id| *aux_id != id);
    }

    pub fn lookup_container_name(&self, name: &str) -> Option<EndpointId> {
        self.container_name.get(name).copied()
    }

    pub fn lookup_container_id(&self, id: &str) -> Option<EndpointId> {
        self.container_id.get(id).copied()
    }

    pub fn lookup_docker_endpoint(&self, id: &str) -> Option<EndpointId> {
        self.docker_endpoint.get(id).copied()
    }

    pub fn lookup_ipv4(&self, ipv4: Ipv4Addr) -> Option<EndpointId> {
        self.ipv4.get(&ipv4).copied()
    }

    pub fn lookup_ipv6(&self, ipv6: Ipv6Addr) -> Option<EndpointId> {
        self.ipv6.get(&ipv6).copied()
    }

    pub fn lookup_pod_name(&self, name: &str) -> Option<EndpointId> {
        self.pod_name.get(name).copied()
    }
}

pub struct EndpointManager {
    inner: RwLock<EndpointManagerInner>,
    pub(crate) id_allocator: IdAllocator,
    pub(crate) loader: Arc<dyn DataplaneLoader>,
    pub(crate) state_dir: PathBuf,
}

impl EndpointManager {
    pub fn new(loader: Arc<dyn DataplaneLoader>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            inner: RwLock::new(EndpointManagerInner::new()),
            id_allocator: IdAllocator::new(),
            loader,
            state_dir: state_dir.into(),
        }
    }

    /// Reads the endpoints persisted in the state directory and reclaims
    /// their ids, then drops whatever the data plane still holds for any
    /// other container: its `lxc_map` entries and its attached programs.
    /// Returns the endpoints to restore (still to be added and regenerated)
    /// and those that could not be restored, whose state directories are
    /// removed.
    pub async fn restore_old_endpoints(&self) -> Res<(Vec<EndpointPersist>, Vec<EndpointPersist>)> {
        let mut restored: Vec<EndpointPersist> = Vec::new();
        let mut to_clean: Vec<EndpointPersist> = Vec::new();

        info!("restoring endpoints...");

        // Even without old endpoints, the data plane may still hold stale
        // state, so carry on and clean it up.
        let possible_endpoints = read_endpoints(&self.state_dir)?;
        if possible_endpoints.is_empty() {
            info!("no old endpoints found.");
        }

        for (id, endpoint) in possible_endpoints {
            match self.loader.link_exists(&endpoint.host_iface_name).await {
                Ok(true) => {}
                Ok(false) => {
                    warn!(
                        "unable to restore endpoint {id}: interface {} is gone",
                        endpoint.host_iface_name
                    );
                    to_clean.push(endpoint);
                    continue;
                }
                // Not a reason to drop its state; regenerating it will
                // fail if the interface really is gone.
                Err(e) => warn!(
                    "unable to check interface {} of endpoint {id}: {e}",
                    endpoint.host_iface_name
                ),
            }
            match self.allocate_reuse_id(id) {
                Ok(lease) => {
                    lease.commit();
                    restored.push(endpoint);
                }
                Err(e) => {
                    warn!("unable to restore endpoint {id}: {e}");
                    to_clean.push(endpoint);
                }
            }
        }

        let keep: HashSet<IpAddr> = restored
            .iter()
            .flat_map(|endpoint| endpoint.ips())
            .collect();
        self.loader.prune_endpoint_infos(keep).await?;

        // The other kinds (e.g. the host) are attached by `setup`.
        let links: HashSet<&str> = restored
            .iter()
            .map(|endpoint| endpoint.host_iface_name.as_str())
            .collect();
        for (kind, link) in self.loader.list_loader_endpoints().await? {
            if kind != LoaderEndpointKind::Container || links.contains(link.as_str()) {
                continue;
            }
            if let Err(e) = self.loader.detach_loader_endpoint(kind, &link).await {
                warn!("unable to detach stale endpoint {link}: {e}");
            }
        }

        for endpoint in &to_clean {
            remove_directories(&self.state_dir, endpoint.id);
        }

        Ok((restored, to_clean))
    }

    pub fn init_metrics(&self) {}

    pub fn add_endpoint(&self, persist: &EndpointPersist) -> Res<Endpoint> {
        let id = persist.id;
        let mut inner = self.inner.write().map_err(|_| {
            DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
        })?;
        if inner.endpoints.contains_key(&id) {
            return Err(DataPlaneError::GeneralError(format!(
                "endpoint {id} already exists"
            )));
        }
        let endpoint = Endpoint::new(self.loader.clone(), self.state_dir.clone(), persist);
        inner.add_endpoint(endpoint.clone());
        inner.update_references(id, persist);
        info!("New Endpoint created: {}", id);
        Ok(endpoint)
    }

    pub fn add_host_endpoint(&self, _endpoint: &EndpointPersist) -> Res<()> {
        Ok(())
    }

    pub async fn delete_endpoint(&self, id: EndpointId, timeout: Option<Duration>) -> Res<()> {
        let ep = {
            let mut inner = self.inner.write().map_err(|_| {
                DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
            })?;
            inner.remove_references(id);
            inner.remove_id(id)
        };
        let shutdown_result = match ep {
            Some(ep) => {
                let is_host = ep.is_host();
                let status = ep.watch_status();
                ep.shutdown();
                // Always wait for the worker, even without a timeout: a
                // regenerate still in its queue would otherwise re-attach the
                // endpoint after it was removed below. A worker aborted on
                // timeout sends nothing more, and anything it already sent to
                // the loader is serialized before the remove.
                let drained = ep.wait_for_shutdown(timeout.unwrap_or(Duration::MAX)).await;
                // The host endpoint is attached by `setup`, not by its worker.
                let removed = if is_host {
                    Ok(())
                } else {
                    let persist = status.borrow().persist.clone();
                    self.loader
                        .remove_loader_endpoint(&LoaderEndpoint::Container(persist))
                        .await
                };
                drained.and(removed)
            }
            None => Ok(()),
        };
        if self.release_id(id).is_err() {
            info!("could not release EndpointId: {id}");
        }
        shutdown_result
    }

    /// Handles to all endpoints, usable after the manager's lock is released.
    pub fn endpoints(&self) -> Vec<Endpoint> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.endpoints.values().cloned().collect()
    }

    pub fn remove_all(&self) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        // Handed-out clones would otherwise keep the workers alive.
        for ep in inner.endpoints.values() {
            ep.shutdown();
        }
        inner.remove_all();
    }

    /// A handle to the endpoint, usable after the manager's lock is released,
    /// e.g. to `regenerate().await` it.
    pub fn endpoint(&self, id: EndpointId) -> Option<Endpoint> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.endpoints.get(&id).cloned()
    }

    pub fn endpoint_exists(&self, id: EndpointId) -> bool {
        let Ok(inner) = self.inner.read() else {
            return false;
        };
        inner.endpoints.contains_key(&id)
    }

    pub fn host_endpoint_exists(&self) -> bool {
        self.host_endpoint_id().is_some()
    }

    pub fn host_endpoint_id(&self) -> Option<EndpointId> {
        let Ok(inner) = self.inner.read() else {
            return None;
        };
        inner
            .endpoints
            .values()
            .find(|endpoint| endpoint.is_host())
            .map(Endpoint::id)
    }

    pub fn update_references(&self, id: EndpointId, endpoint: &EndpointPersist) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.update_references(id, endpoint)
    }

    pub fn remove_references(&self, id: EndpointId) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.remove_references(id)
    }

    pub fn lookup(&self, id: EndpointIdentification) -> Option<EndpointId> {
        match id {
            EndpointIdentification::ContainerId(id) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_container_id(&id)
            }
            EndpointIdentification::ContainerName(name) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_container_name(&name)
            }
            EndpointIdentification::DockerEndpoint(endpoint) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_docker_endpoint(&endpoint)
            }
            EndpointIdentification::Ipv4(ipv4) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_ipv4(ipv4)
            }
            EndpointIdentification::Ipv6(ipv6) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_ipv6(ipv6)
            }
            EndpointIdentification::Local(endpoint_id) => Some(endpoint_id),
            EndpointIdentification::PodName(name) => {
                let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
                inner.lookup_pod_name(&name)
            }
        }
    }

    pub fn lookup_container_id(&self, id: &str) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_container_id(id)
    }

    pub fn lookup_container_name(&self, name: &str) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_container_name(name)
    }

    pub fn lookup_docker_endpoint(&self, name: &str) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_docker_endpoint(name)
    }

    pub fn lookup_ip(&self, ip: IpAddr) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        match ip {
            IpAddr::V4(ipv4) => inner.lookup_ipv4(ipv4),
            IpAddr::V6(ipv6) => inner.lookup_ipv6(ipv6),
        }
    }

    pub fn lookup_ipv4(&self, ipv4: Ipv4Addr) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_ipv4(ipv4)
    }

    pub fn lookup_ipv6(&self, ipv6: Ipv6Addr) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_ipv6(ipv6)
    }

    pub fn lookup_pod_name(&self, name: &str) -> Option<EndpointId> {
        let inner = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        inner.lookup_pod_name(name)
    }

    pub fn allocate_new_id(&self) -> Res<IdLease<'_>> {
        let id = self.id_allocator.allocate().ok_or_else(|| {
            DataPlaneError::GeneralError("no more EndpointId's available".to_string())
        })?;
        Ok(IdLease::new(&self.id_allocator, id))
    }

    pub fn allocate_reuse_id(&self, current_id: EndpointId) -> Res<IdLease<'_>> {
        self.id_allocator.reuse(current_id).map_err(|e| {
            DataPlaneError::GeneralError(format!("unable to reuse EndpointId: {e}"))
        })?;
        Ok(IdLease::new(&self.id_allocator, current_id))
    }

    fn release_id(&self, id: EndpointId) -> Res<()> {
        self.id_allocator.release(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EndpointSpec, LoaderCall, MockDataplaneLoader, endpoint::directory::state_directory_path,
    };

    #[tokio::test]
    async fn delete_endpoint_removes_it_from_the_loader_after_queued_work() {
        let dir = std::env::temp_dir().join(format!("sarena-mgr-delete-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let loader = Arc::new(MockDataplaneLoader::new());
        let manager = EndpointManager::new(loader.clone(), &dir);

        let persist = EndpointPersist {
            id: EndpointId::new(3),
            spec: EndpointSpec {
                host_iface_name: "lxc3".to_string(),
                ..Default::default()
            },
        };
        manager
            .add_endpoint(&persist)
            .unwrap()
            .regenerate()
            .await
            .unwrap();
        manager.delete_endpoint(persist.id, None).await.unwrap();

        let kind = LoaderEndpointKind::Container;
        assert_eq!(
            loader.calls(),
            vec![
                LoaderCall::AddLoaderEndpoint(kind, "lxc3".to_string()),
                LoaderCall::RemoveLoaderEndpoint(kind, "lxc3".to_string()),
            ]
        );
        assert!(!manager.endpoint_exists(persist.id));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn restore_reclaims_persisted_endpoints_and_drops_stale_containers() {
        let dir = std::env::temp_dir().join(format!("sarena-mgr-restore-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let persist = EndpointPersist {
            id: EndpointId::new(3),
            spec: EndpointSpec {
                host_iface_name: "lxc3".to_string(),
                ipv4: Some(Ipv4Addr::new(10, 0, 0, 3)),
                ..Default::default()
            },
        };
        let state = state_directory_path(&dir, persist.id);
        std::fs::create_dir_all(&state).unwrap();
        persist.write_state(&state).unwrap();

        let loader = Arc::new(MockDataplaneLoader::new());
        let host = LoaderEndpoint::Host {
            link: "sarena_host".to_string(),
        };
        let stale = LoaderEndpoint::Container(EndpointPersist {
            id: EndpointId::new(4),
            spec: EndpointSpec {
                host_iface_name: "lxc4".to_string(),
                ..Default::default()
            },
        });
        for endpoint in [&host, &LoaderEndpoint::Container(persist.clone()), &stale] {
            loader.add_loader_endpoint(endpoint).await.unwrap();
        }
        let manager = EndpointManager::new(loader.clone(), &dir);

        let (restored, to_clean) = manager.restore_old_endpoints().await.unwrap();

        assert_eq!(restored, vec![persist.clone()]);
        assert_eq!(to_clean, vec![]);
        assert!(manager.allocate_reuse_id(persist.id).is_err());
        assert!(
            loader
                .calls()
                .contains(&LoaderCall::PruneEndpointInfos(vec![IpAddr::V4(
                    Ipv4Addr::new(10, 0, 0, 3)
                )]))
        );
        assert_eq!(
            loader.endpoints(),
            vec![
                (LoaderEndpointKind::Host, "sarena_host".to_string()),
                (LoaderEndpointKind::Container, "lxc3".to_string()),
            ]
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn restore_cleans_up_endpoints_whose_interface_is_gone() {
        let dir = std::env::temp_dir().join(format!("sarena-mgr-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let persist = EndpointPersist {
            id: EndpointId::new(5),
            spec: EndpointSpec {
                host_iface_name: "lxc5".to_string(),
                ipv4: Some(Ipv4Addr::new(10, 0, 0, 5)),
                ..Default::default()
            },
        };
        let state = state_directory_path(&dir, persist.id);
        std::fs::create_dir_all(&state).unwrap();
        persist.write_state(&state).unwrap();

        let loader = Arc::new(MockDataplaneLoader::new());
        loader
            .add_loader_endpoint(&LoaderEndpoint::Container(persist.clone()))
            .await
            .unwrap();
        loader.remove_link("lxc5");
        let manager = EndpointManager::new(loader.clone(), &dir);

        let (restored, to_clean) = manager.restore_old_endpoints().await.unwrap();

        assert_eq!(restored, vec![]);
        assert_eq!(to_clean, vec![persist.clone()]);
        assert!(!state.exists());
        // Its id was never reclaimed.
        manager.allocate_reuse_id(persist.id).unwrap().commit();
        assert!(
            loader
                .calls()
                .contains(&LoaderCall::PruneEndpointInfos(vec![]))
        );
        assert_eq!(loader.endpoints(), vec![]);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
