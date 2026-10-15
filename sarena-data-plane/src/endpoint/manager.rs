use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{PoisonError, RwLock, RwLockReadGuard},
    time::Duration,
};

use tracing::info;

use super::{Endpoint, EndpointId, id_allocator::IdAllocator, worker::EndpointPersist};
use crate::{DataPlaneError, Res, endpoint::id::EndpointIdentification};

struct EndpointManagerInner {
    endpoints: HashMap<EndpointId, Endpoint>,
    endpoints_container: HashMap<EndpointIdentification, EndpointId>,
}

impl EndpointManagerInner {
    pub fn new() -> Self {
        Self {
            endpoints: HashMap::new(),
            endpoints_container: HashMap::new(),
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
        self.endpoints_container.clear();
    }

    pub fn update_references(&mut self, _id: EndpointId, _endpoint: &EndpointPersist) {
        // TODO
    }

    pub fn remove_references(&mut self, id: EndpointId) {
        self.endpoints_container.retain(|_, aux_id| *aux_id != id);
    }
}

pub struct EndpointManager {
    inner: RwLock<EndpointManagerInner>,
    id_allocator: IdAllocator,
}

impl EndpointManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(EndpointManagerInner::new()),
            id_allocator: IdAllocator::new(),
        }
    }

    pub fn init_metrics(&self) {}

    pub fn add_endpoint(
        &self,
        id: Option<EndpointId>,
        persist: &EndpointPersist,
        state_dir: impl Into<PathBuf>,
    ) -> Res<()> {
        let id = self.allocate_id(id)?;
        let mut inner = self.inner.write().map_err(|_| {
            DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
        })?;
        if inner.endpoints.contains_key(&persist.id) {
            return Err(DataPlaneError::GeneralError(format!(
                "endpoint {:?} already exists",
                persist.id
            )));
        }
        let endpoint = Endpoint::new(persist, state_dir)?;
        inner.add_endpoint(endpoint);
        inner.update_references(id, persist);

        info!("New Endpoint created: {}", id);
        Ok(())
    }

    pub fn add_host_endpoint(&self, _endpoint: &EndpointPersist) -> Res<()> {
        Ok(())
    }

    pub async fn remove_endpoint(&self, id: EndpointId, timeout: Option<Duration>) -> Res<()> {
        let mut inner = self.inner.write().map_err(|_| {
            DataPlaneError::GeneralError("endpoint manager lock poisoned".to_string())
        })?;
        if let Some(ep) = inner.remove_id(id) {
            ep.shutdown();
            if let Some(d) = timeout {
                ep.wait_for_shutdown(d).await?;
            }
        }
        if self.release_id(id).is_err() {
            info!("could not release EndpointId: {id}");
        }
        inner.remove_references(id);
        Ok(())
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
        let Ok(inner) = self.inner.read() else {
            return false;
        };
        inner.endpoints.values().any(Endpoint::is_host)
    }

    pub fn update_references(&self, id: EndpointId, endpoint: &EndpointPersist) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.update_references(id, endpoint)
    }

    pub fn remove_references(&self, id: EndpointId) {
        let mut inner = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        inner.remove_references(id)
    }

    fn allocate_id(&self, current_id: Option<EndpointId>) -> Res<EndpointId> {
        match current_id {
            Some(id) => {
                self.id_allocator.reuse(id).map_err(|e| {
                    DataPlaneError::GeneralError(format!("unable to reuse EndpointId: {e}"))
                })?;
                Ok(id)
            }
            None => self.id_allocator.allocate().ok_or_else(|| {
                DataPlaneError::GeneralError("no more EndpointId's available".to_string())
            }),
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
