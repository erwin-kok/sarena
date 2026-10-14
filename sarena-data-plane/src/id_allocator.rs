use core::fmt;
use std::{
    collections::HashSet,
    sync::{Mutex, MutexGuard, PoisonError},
};

use serde::{Deserialize, Serialize};

use crate::{DataPlaneError, Res};

const MIN_ID: EndpointId = EndpointId::new(1);
const MAX_ID: EndpointId = EndpointId::new(4095);

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointId(u64);

impl EndpointId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for EndpointId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<EndpointId> for u64 {
    fn from(id: EndpointId) -> Self {
        id.0
    }
}

impl fmt::Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

pub struct IdAllocator {
    cache: Mutex<IdCache>,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(IdCache::new(MIN_ID, MAX_ID)),
        }
    }

    pub fn allocate(&self) -> Option<EndpointId> {
        self.cache().allocate_id()
    }

    pub fn reuse(&self, id: EndpointId) -> Res<()> {
        if id < MIN_ID {
            return Err(DataPlaneError::GeneralError(format!(
                "unable to reuse EndpointId: {id} < {MIN_ID}"
            )));
        }
        if id > MAX_ID {
            return Ok(());
        }
        if !self.cache().remove(id) {
            return Err(DataPlaneError::GeneralError(format!(
                "EndpointId {id} is already in use"
            )));
        }
        Ok(())
    }

    pub fn release(&self, id: EndpointId) -> Res<()> {
        if !self.cache().insert(id) {
            return Err(DataPlaneError::GeneralError(format!(
                "unable to release EndpointId {id}"
            )));
        }
        Ok(())
    }

    fn cache(&self) -> MutexGuard<'_, IdCache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Debug)]
struct IdCache {
    /// IDs available in this cache.
    ids: HashSet<EndpointId>,
    /// IDs currently leased from this cache.
    leased: HashSet<EndpointId>,
}

impl IdCache {
    fn new(min_id: EndpointId, max_id: EndpointId) -> Self {
        Self {
            ids: (min_id.0..=max_id.0).map(EndpointId).collect(),
            leased: HashSet::new(),
        }
    }

    fn allocate_id(&mut self) -> Option<EndpointId> {
        let id = *self.ids.iter().next()?;
        self.ids.remove(&id);
        Some(id)
    }

    fn insert(&mut self, id: EndpointId) -> bool {
        !self.leased.contains(&id) && self.ids.insert(id)
    }

    fn remove(&mut self, id: EndpointId) -> bool {
        self.leased.remove(&id);
        self.ids.remove(&id)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::mpsc, thread};

    use super::*;

    #[test]
    fn allocation() {
        let allocator = IdAllocator::new();
        let mut set = HashSet::new();
        for _ in MIN_ID.get()..=MAX_ID.get() {
            let id = allocator.allocate().expect("allocate should succeed");
            assert!(set.insert(id));
        }
        assert!(allocator.allocate().is_none());
    }

    #[test]
    fn reuse() {
        let allocator = IdAllocator::new();
        assert!(allocator.reuse(EndpointId::new(MAX_ID.get() + 10)).is_ok());
        assert!(allocator.reuse(EndpointId::new(MIN_ID.get() - 1)).is_err());

        let mut ids_returned = HashSet::new();

        assert!(allocator.reuse(EndpointId::new(2)).is_ok());
        ids_returned.insert(EndpointId::new(2));

        assert!(allocator.reuse(EndpointId::new(8)).is_ok());
        ids_returned.insert(EndpointId::new(8));

        for _ in MIN_ID.get()..=MAX_ID.get() - 2 {
            let id = allocator.allocate().expect("allocate should succeed");
            assert!(ids_returned.insert(id), "ID {id} returned more than once");
        }

        assert!(allocator.allocate().is_none());
        assert!(allocator.reuse(EndpointId::new(2)).is_err());
        assert!(allocator.reuse(EndpointId::new(3)).is_err());
        assert!(allocator.release(EndpointId::new(5)).is_ok());

        ids_returned.remove(&EndpointId::new(5));
        assert!(allocator.release(EndpointId::new(6)).is_ok());

        ids_returned.remove(&EndpointId::new(6));
        assert!(allocator.reuse(EndpointId::new(5)).is_ok());

        ids_returned.insert(EndpointId::new(5));
        assert_eq!(allocator.allocate(), Some(EndpointId::new(6)));
    }

    #[test]
    fn release() {
        let allocator = IdAllocator::new();

        for i in MIN_ID.get()..=MAX_ID.get() {
            assert!(allocator.reuse(EndpointId::new(i)).is_ok());
        }

        assert!(allocator.allocate().is_none());

        for i in MIN_ID.get()..=MAX_ID.get() {
            assert!(allocator.release(EndpointId::new(i)).is_ok());
        }
    }

    fn allocate_all(allocator: &IdAllocator) -> HashSet<EndpointId> {
        let mut ids = HashSet::new();
        while let Some(id) = allocator.allocate() {
            assert!(ids.insert(id), "ID {id} allocated more than once");
        }
        ids
    }

    #[test]
    fn allocated_ids_cover_range() {
        let allocator = IdAllocator::new();
        let ids = allocate_all(&allocator);
        let expected: HashSet<_> = (MIN_ID.get()..=MAX_ID.get()).map(EndpointId::new).collect();
        assert_eq!(ids, expected);
    }

    #[test]
    fn released_id_is_allocated_again() {
        let allocator = IdAllocator::new();
        allocate_all(&allocator);

        assert!(allocator.release(EndpointId::new(42)).is_ok());
        assert_eq!(allocator.allocate(), Some(EndpointId::new(42)));
        assert!(allocator.allocate().is_none());
    }

    #[test]
    fn release_of_available_id_fails() {
        let allocator = IdAllocator::new();
        assert!(allocator.release(MIN_ID).is_err());
        assert!(allocator.release(MAX_ID).is_err());
        assert_eq!(allocate_all(&allocator).len() as u64, MAX_ID.get());
    }

    #[test]
    fn double_release_fails() {
        let allocator = IdAllocator::new();
        let id = allocator.allocate().expect("allocate should succeed");
        assert!(allocator.release(id).is_ok());
        assert!(allocator.release(id).is_err());
    }

    #[test]
    fn reuse_of_allocated_id_fails() {
        let allocator = IdAllocator::new();
        let id = allocator.allocate().expect("allocate should succeed");
        assert!(allocator.reuse(id).is_err());
    }

    #[test]
    fn reuse_release_round_trip() {
        let allocator = IdAllocator::new();
        let id = EndpointId::new(100);

        assert!(allocator.reuse(id).is_ok());
        assert!(allocator.reuse(id).is_err());
        assert!(!allocate_all(&allocator).contains(&id));

        assert!(allocator.release(id).is_ok());
        assert!(allocator.reuse(id).is_ok());
        assert!(allocator.allocate().is_none());
    }

    #[test]
    fn reuse_boundaries() {
        let allocator = IdAllocator::new();
        assert!(allocator.reuse(EndpointId::new(0)).is_err());
        assert!(allocator.reuse(MIN_ID).is_ok());
        assert!(allocator.reuse(MAX_ID).is_ok());
        assert!(allocator.reuse(EndpointId::new(MAX_ID.get() + 1)).is_ok());
        assert!(allocator.reuse(EndpointId::new(u64::MAX)).is_ok());

        let ids = allocate_all(&allocator);
        assert_eq!(ids.len() as u64, MAX_ID.get() - 2);
        assert!(!ids.contains(&MIN_ID));
        assert!(!ids.contains(&MAX_ID));
    }

    #[test]
    fn reuse_above_max_is_repeatable() {
        let allocator = IdAllocator::new();
        let id = EndpointId::new(MAX_ID.get() + 1);
        assert!(allocator.reuse(id).is_ok());
        assert!(allocator.reuse(id).is_ok());
    }

    #[test]
    fn concurrent_allocation_yields_unique_ids() {
        const THREADS: usize = 8;

        let allocator = IdAllocator::new();
        let (tx, rx) = mpsc::channel();
        thread::scope(|s| {
            for _ in 0..THREADS {
                let tx = tx.clone();
                let allocator = &allocator;
                s.spawn(move || {
                    while let Some(id) = allocator.allocate() {
                        tx.send(id).unwrap();
                    }
                });
            }
        });
        drop(tx);

        let mut ids = HashSet::new();
        for id in rx {
            assert!(ids.insert(id), "ID {id} allocated more than once");
        }
        assert_eq!(ids.len() as u64, MAX_ID.get());
    }

    #[test]
    fn concurrent_release_and_allocate() {
        const THREADS: u64 = 8;

        let allocator = IdAllocator::new();
        let ids: Vec<_> = allocate_all(&allocator).into_iter().collect();
        thread::scope(|s| {
            for chunk in ids.chunks(ids.len().div_ceil(THREADS as usize)) {
                let allocator = &allocator;
                s.spawn(move || {
                    for &id in chunk {
                        allocator.release(id).expect("release should succeed");
                    }
                });
            }
        });

        assert_eq!(allocate_all(&allocator).len(), ids.len());
    }
}
