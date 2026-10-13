use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::asyncprim::{
    Termination,
    controller::{ControllerObserver, ControllerStatus, Runner, Spec, StatusCell},
    util::lock,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL: LazyLock<Registry> = LazyLock::new(Registry::new);

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

    fn insert(&self, id: u64, cell: &Arc<StatusCell>) {
        lock(&self.inner).insert(id, Arc::downgrade(cell));
    }

    fn remove(&self, id: u64) {
        lock(&self.inner).remove(&id);
    }

    pub fn statuses(&self) -> Vec<ControllerStatus> {
        let mut map = lock(&self.inner);
        map.retain(|_, cell| cell.strong_count() > 0);
        let mut out: Vec<_> = map
            .values()
            .filter_map(Weak::upgrade)
            .map(|cell| cell.snapshot())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        out
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("controllers", &lock(&self.inner).len())
            .finish()
    }
}

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

struct Entry {
    id: u64,
    spec_tx: watch::Sender<Spec>,
    stop: CancellationToken,
    done: CancellationToken,
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
        let mut entries = lock(&self.entries);

        if let Some(entry) = entries.get(&name) {
            entry.status.configure(&spec);
            entry.spec_tx.send_replace(spec);
            return;
        }

        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let status = Arc::new(StatusCell::new(&name, id, &spec));
        let (spec_tx, spec_rx) = watch::channel(spec);
        let stop = self.root.child_token();
        let done = CancellationToken::new();
        self.registry.insert(id, &status);

        let runner = Runner {
            name: name.clone(),
            spec_rx,
            stop: stop.clone(),
            status: status.clone(),
            observer: self.observer.clone(),
        };
        // The guard fires `done` when the task ends, even if it panics or the
        // runtime drops it.
        let guard = done.clone().drop_guard();
        tokio::spawn(async move {
            let _guard = guard;
            runner.run().await;
        });

        entries.insert(
            name,
            Entry {
                id,
                spec_tx,
                stop,
                done,
                status,
            },
        );
    }

    /// Stops a controller. Await the result to wait until its stop hook has
    /// run. Returns `None` for an unknown name.
    pub fn remove(&self, name: &str) -> Option<Termination> {
        let entry = lock(&self.entries).remove(name)?;
        Some(self.stop_entry(entry))
    }

    /// Stops every controller in this manager.
    pub fn remove_all(&self) -> Termination {
        let entries: Vec<Entry> = lock(&self.entries).drain().map(|(_, e)| e).collect();
        Termination::all(entries.into_iter().map(|e| self.stop_entry(e)).collect())
    }

    /// Resolves when the named controller has terminated; resolves at once
    /// for an unknown name.
    pub fn termination(&self, name: &str) -> Termination {
        match lock(&self.entries).get(name) {
            Some(entry) => Termination::from_token(entry.done.clone()),
            None => Termination::ready(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        lock(&self.entries).contains_key(name)
    }

    /// The status of this manager's controllers, sorted by name.
    pub fn statuses(&self) -> Vec<ControllerStatus> {
        let mut out: Vec<_> = lock(&self.entries)
            .values()
            .map(|e| e.status.snapshot())
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn stop_entry(&self, entry: Entry) -> Termination {
        entry.stop.cancel();
        self.registry.remove(entry.id);
        Termination::from_token(entry.done)
    }

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
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.root.cancel();
        for entry in lock(&self.entries).values() {
            self.registry.remove(entry.id);
        }
    }
}

impl fmt::Debug for Manager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut names: Vec<String> = lock(&self.entries).keys().cloned().collect();
        names.sort();
        f.debug_struct("Manager")
            .field("controllers", &names)
            .finish()
    }
}
