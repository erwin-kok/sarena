use std::{
    collections::HashMap,
    sync::{RwLock, RwLockReadGuard},
};

use crate::{
    DataPlaneError, Res,
    endpoint::Endpoint,
    id_allocator::{EndpointId, IdAllocator},
};

pub struct EndpointManager {
    endpoints: RwLock<HashMap<EndpointId, Endpoint>>,
    id_allocator: IdAllocator,
}

impl Default for EndpointManager {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointManager {
    pub fn new() -> Self {
        Self {
            endpoints: RwLock::new(HashMap::new()),
            id_allocator: IdAllocator::new(),
        }
    }

    pub fn add_endpoint(_endpoint: Endpoint) -> Res<()> {
        Ok(())
    }

    pub fn endpoints(&self) -> EndpointsGuard<'_> {
        EndpointsGuard {
            guard: self.endpoints.read().expect("RwLock poisoned"),
        }
    }

    pub fn allocate_id(&self, current_id: Option<EndpointId>) -> Res<EndpointId> {
        match current_id {
            Some(id) => {
                self.id_allocator.reuse(id)?;
                Ok(id)
            }
            None => self.id_allocator.allocate().ok_or_else(|| {
                DataPlaneError::GeneralError("no more EndpointId's available".to_string())
            }),
        }
    }

    pub fn release_id(&self, id: EndpointId) -> Res<()> {
        self.id_allocator.release(id)
    }

    pub fn update_id(&self, _ep: &Endpoint) {}

    pub fn remove_id(&self, id: EndpointId) {
        self.endpoints.write().expect("RwLock poisoned").remove(&id);
    }
}

pub struct EndpointsGuard<'a> {
    guard: RwLockReadGuard<'a, HashMap<EndpointId, Endpoint>>,
}

impl EndpointsGuard<'_> {
    pub fn iter(&self) -> impl Iterator<Item = (&EndpointId, &Endpoint)> + '_ {
        self.guard.iter()
    }

    pub fn values(&self) -> impl Iterator<Item = &Endpoint> + '_ {
        self.guard.values()
    }
}
