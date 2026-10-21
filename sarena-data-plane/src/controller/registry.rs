use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, LazyLock, Mutex, Weak},
};

use crate::controller::status::{ControllerStatus, StatusCell};

static GLOBAL: LazyLock<Registry> = LazyLock::new(Registry::new);

/// An index of controller statuses across managers, keyed by controller id.
/// It holds weak references, so it never keeps a controller's status alive.
#[derive(Clone, Default)]
pub struct Registry {
    inner: Arc<Mutex<HashMap<u64, Weak<StatusCell>>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide registry that managers use unless told otherwise.
    pub fn global() -> &'static Registry {
        &GLOBAL
    }

    pub(crate) fn insert(&self, id: u64, cell: &Arc<StatusCell>) {
        self.lock().insert(id, Arc::downgrade(cell));
    }

    pub(crate) fn remove(&self, id: u64) {
        self.lock().remove(&id);
    }

    /// The status of every live controller, sorted by name, then id.
    pub fn statuses(&self) -> Vec<ControllerStatus> {
        let mut map = self.lock();
        map.retain(|_, cell| cell.strong_count() > 0);
        let mut out: Vec<_> = map
            .values()
            .filter_map(Weak::upgrade)
            .map(|cell| cell.snapshot())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Weak<StatusCell>>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("controllers", &self.lock().len())
            .finish()
    }
}
