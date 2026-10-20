use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
    sync::{PoisonError, RwLock, RwLockReadGuard},
    time::Duration,
};

use tracing::info;

use super::{
    Endpoint, EndpointId,
    id_allocator::{IdAllocator, IdLease},
};
use crate::{
    DataPlaneError, EndpointPersist, LoaderHandle, Res, endpoint::id::EndpointIdentification,
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
    pub(crate) loader_handle: LoaderHandle,
}

impl EndpointManager {
    pub fn new(loader_handle: LoaderHandle) -> Self {
        Self {
            inner: RwLock::new(EndpointManagerInner::new()),
            id_allocator: IdAllocator::new(),
            loader_handle,
        }
    }

    pub fn init_metrics(&self) {}

    pub fn add_endpoint(
        &self,
        persist: &EndpointPersist,
        state_dir: impl Into<PathBuf>,
    ) -> Res<()> {
        let id = persist.id;
        let mut inner = self.inner.write().map_err(|_| {
            DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
        })?;
        if inner.endpoints.contains_key(&id) {
            return Err(DataPlaneError::GeneralError(format!(
                "endpoint {id} already exists"
            )));
        }
        let endpoint = Endpoint::new(persist, state_dir);
        inner.add_endpoint(endpoint);
        inner.update_references(id, persist);
        info!("New Endpoint created: {}", id);
        Ok(())
    }

    pub fn add_host_endpoint(&self, _endpoint: &EndpointPersist) -> Res<()> {
        Ok(())
    }

    pub async fn remove_endpoint(&self, id: EndpointId, timeout: Option<Duration>) -> Res<()> {
        let ep = {
            let mut inner = self.inner.write().map_err(|_| {
                DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
            })?;
            inner.remove_references(id);
            inner.remove_id(id)
        };
        let shutdown_result = match ep {
            Some(ep) => {
                ep.shutdown();
                match timeout {
                    Some(d) => ep.wait_for_shutdown(d).await,
                    None => Ok(()),
                }
            }
            None => Ok(()),
        };
        if self.release_id(id).is_err() {
            info!("could not release EndpointId: {id}");
        }
        shutdown_result
    }

    pub fn endpoints(&self) -> EndpointsGuard<'_> {
        EndpointsGuard {
            guard: self.inner.read().expect("RwLock poisoned"),
        }
    }

    pub fn remove_all(&self) {
        self.inner
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove_all();
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

    pub(crate) fn allocate_new_id(&self) -> Res<IdLease<'_>> {
        let id = self.id_allocator.allocate().ok_or_else(|| {
            DataPlaneError::GeneralError("no more EndpointId's available".to_string())
        })?;
        Ok(IdLease::new(&self.id_allocator, id))
    }

    pub(crate) fn allocate_id(&self, current_id: Option<EndpointId>) -> Res<IdLease<'_>> {
        match current_id {
            Some(id) => {
                self.id_allocator.reuse(id).map_err(|e| {
                    DataPlaneError::GeneralError(format!("unable to reuse EndpointId: {e}"))
                })?;
                Ok(IdLease::new(&self.id_allocator, id))
            }
            None => self.allocate_new_id(),
        }
    }

    fn release_id(&self, id: EndpointId) -> Res<()> {
        self.id_allocator.release(id)
    }
}

pub struct EndpointsGuard<'a> {
    guard: RwLockReadGuard<'a, EndpointManagerInner>,
}

impl EndpointsGuard<'_> {
    pub fn iter(&self) -> impl Iterator<Item = (&EndpointId, &Endpoint)> + '_ {
        self.guard.endpoints.iter()
    }

    pub fn values(&self) -> impl Iterator<Item = &Endpoint> + '_ {
        self.guard.endpoints.values()
    }
}
