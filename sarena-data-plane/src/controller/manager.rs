use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::controller::{
    registry::Registry,
    runner::Runner,
    spec::Spec,
    status::{ControllerObserver, ControllerStatus, StatusCell},
    termination::Termination,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct ManagerBuilder {
    registry: Registry,
    max_interval: Option<Duration>,
    observer: Option<Arc<dyn ControllerObserver>>,
}

impl ManagerBuilder {
    #[must_use]
    pub fn registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    /// Caps every controller's interval.
    #[must_use]
    pub fn max_interval(mut self, max: Duration) -> Self {
        self.max_interval = Some(max);
        self
    }

    #[must_use]
    pub fn observer(mut self, observer: Arc<dyn ControllerObserver>) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn build(self) -> Manager {
        Manager {
            entries: Mutex::new(HashMap::new()),
            root: CancellationToken::new(),
            registry: self.registry,
            max_interval: self.max_interval,
            observer: self.observer,
        }
    }
}

/// The manager's handle on one running controller.
struct Entry {
    id: u64,
    spec_tx: watch::Sender<Spec>,
    /// Cancelled to stop the controller.
    cancel: CancellationToken,
    /// Cancelled once the controller task has ended, stop hook included.
    terminated: CancellationToken,
    status: Arc<StatusCell>,
}

/// A set of named controllers. Dropping the manager stops all of them.
pub struct Manager {
    entries: Mutex<HashMap<String, Entry>>,
    root: CancellationToken,
    registry: Registry,
    max_interval: Option<Duration>,
    observer: Option<Arc<dyn ControllerObserver>>,
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    pub fn new() -> Self {
        Self::builder().build()
    }

    pub fn builder() -> ManagerBuilder {
        ManagerBuilder {
            registry: Registry::global().clone(),
            max_interval: None,
            observer: None,
        }
    }

    /// Creates the controller and starts it, or replaces the spec of an
    /// existing controller and runs its job again at once. Updates that
    /// arrive while a run is in progress are folded into one rerun.
    pub fn update(&self, name: impl Into<String>, spec: Spec) {
        let name = name.into();
        let spec = self.normalize(spec);
        let mut entries = self.lock_entries();

        if let Some(entry) = entries.get(&name) {
            entry.status.configure(&spec);
            entry.spec_tx.send_replace(spec);
            return;
        }

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let status = Arc::new(StatusCell::new(&name, id, &spec));
        let (spec_tx, spec_rx) = watch::channel(spec);
        let cancel = self.root.child_token();
        let terminated = CancellationToken::new();
        self.registry.insert(id, &status);

        let runner = Runner::new(
            name.clone(),
            spec_rx,
            cancel.clone(),
            status.clone(),
            self.observer.clone(),
        );
        // The guard fires `terminated` when the task ends, even if it panics
        // or the runtime drops it.
        let guard = terminated.clone().drop_guard();
        tokio::spawn(async move {
            let _guard = guard;
            runner.run().await;
        });

        entries.insert(
            name,
            Entry {
                id,
                spec_tx,
                cancel,
                terminated,
                status,
            },
        );
    }

    /// Stops a controller. Await the result to wait until its stop hook has
    /// run. Returns `None` for an unknown name.
    pub fn remove(&self, name: &str) -> Option<Termination> {
        let entry = self.lock_entries().remove(name)?;
        Some(self.stop_entry(entry))
    }

    /// Stops every controller in this manager.
    pub fn remove_all(&self) -> Termination {
        let entries: Vec<Entry> = self.lock_entries().drain().map(|(_, e)| e).collect();
        Termination::all(entries.into_iter().map(|e| self.stop_entry(e)).collect())
    }

    /// Resolves when the named controller has terminated; resolves at once
    /// for an unknown name.
    pub fn termination(&self, name: &str) -> Termination {
        match self.lock_entries().get(name) {
            Some(entry) => Termination::from_token(entry.terminated.clone()),
            None => Termination::ready(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.lock_entries().contains_key(name)
    }

    /// The status of this manager's controllers, sorted by name.
    pub fn statuses(&self) -> Vec<ControllerStatus> {
        let mut out: Vec<_> = self
            .lock_entries()
            .values()
            .map(|e| e.status.snapshot())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn stop_entry(&self, entry: Entry) -> Termination {
        entry.cancel.cancel();
        self.registry.remove(entry.id);
        Termination::from_token(entry.terminated)
    }

    /// Treats a zero interval as none and caps it at `max_interval`.
    fn normalize(&self, mut spec: Spec) -> Spec {
        spec.interval = spec.interval.filter(|d| !d.is_zero());
        if let (Some(interval), Some(max)) = (spec.interval, self.max_interval)
            && interval > max
        {
            tracing::info!(?max, "limiting controller interval");
            spec.interval = Some(max);
        }
        spec
    }

    fn lock_entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.root.cancel();
        for entry in self.lock_entries().values() {
            self.registry.remove(entry.id);
        }
    }
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<String> = self.lock_entries().keys().cloned().collect();
        names.sort();
        f.debug_struct("Manager")
            .field("controllers", &names)
            .field("max_interval", &self.max_interval)
            .field("has_observer", &self.observer.is_some())
            .finish_non_exhaustive()
    }
}
