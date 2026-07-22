use std::{
    fs::File,
    future::Future,
    os::fd::AsFd,
    path::{Path, PathBuf},
    sync::Mutex,
};

use futures::TryStreamExt;
use netlink_packet_route::link::{LinkFlags, LinkMessage};
use nix::{
    fcntl::{Flock, FlockArg},
    mount::{MsFlags, mount, umount},
    sched::{CloneFlags, unshare},
};

use crate::{InfraError, Res};

const NETNS_PATH: &str = "/run/netns";

/// The *calling thread's* current network namespace.
///
/// Deliberately **not** `/proc/self/ns/net`: `/proc/self` resolves to the
/// thread-group leader (the process's main thread), not the calling
/// thread, so it would report the wrong namespace for any non-main thread.
/// `/proc/thread-self` (Linux >= 3.17) is the per-thread equivalent and is
/// what we actually need here, since `setns`/`unshare` are thread-local.
const THREAD_SELF_NS_PATH: &str = "/proc/thread-self/ns/net";

/// Serializes the one-time bootstrap of `/run/netns` itself against other
/// callers *within this process*. Cross-process safety is provided
/// separately by an flock on a lock file inside that directory (see
/// [`ensure_netns_dir`]).
static NETNS_DIR_BOOTSTRAP: Mutex<()> = Mutex::new(());

/// A named network namespace that closures can be executed inside.
pub struct Netns {
    pub path: PathBuf,
    pub fd: File,
}

impl Netns {
    /// Open the namespace `/run/netns/<name>`.
    pub fn open(name: &str) -> Res<Self> {
        let path = Path::new(NETNS_PATH).join(name);
        let fd = File::open(&path).map_err(|e| InfraError::OpenNamespace {
            name: name.to_owned(),
            source: e,
        })?;
        Ok(Self { path, fd })
    }

    /// Open a network namespace by an arbitrary absolute path
    /// (e.g. `/proc/<pid>/ns/net`).
    pub fn open_path(path: impl AsRef<Path>) -> Res<Self> {
        let path = path.as_ref().to_path_buf();
        let fd = File::open(&path).map_err(|e| InfraError::OpenNamespace {
            name: path.display().to_string(),
            source: e,
        })?;
        Ok(Self { path, fd })
    }

    /// Create a new, persistent network namespace named `name`, reachable
    /// afterwards via `Netns::open(name)`.
    ///
    /// This provisions **only** a loopback interface brought up (`lo`); every
    /// fresh Linux network namespace has one, but it starts down. Anything
    /// beyond that -- in particular, giving the namespace a way to reach
    /// anything else -- is a separate, explicit step: see
    /// [`crate::link::create_veth_pair`] and [`crate::network::LinkHandle::set_ns`].
    ///
    /// Fails with [`InfraError::NamespaceExists`] if `name` is already taken.
    /// On any failure after the namespace file is created, this rolls back
    /// (unmounts/removes it) rather than leaving a broken entry behind.
    pub async fn create(name: &str) -> Res<()> {
        ensure_netns_dir()?;

        let path = Path::new(NETNS_PATH).join(name);
        if path.exists() {
            return Err(InfraError::NamespaceExists(name.to_owned()));
        }

        std::fs::File::create(&path).map_err(|e| InfraError::Io {
            context: format!("create {}", path.display()),
            source: e,
        })?;

        let path_for_thread = path.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name(format!("netns-create-{name}"))
            .spawn(move || {
                let outcome = create_netns_on_thread(&path_for_thread);
                let _ = tx.send(outcome);
            })
            .map_err(InfraError::SpawnThread)?;

        let result = rx.await.map_err(|_| InfraError::ThreadPanicked)?;

        if let Err(err) = result {
            rollback_netns_file(&path);
            return Err(err);
        }

        Ok(())
    }

    /// Delete the namespace named `name`, created previously via
    /// [`create_netns`] (or `ip netns add`).
    ///
    /// This does not itself require thread pinning: unlike `setns`/`unshare`,
    /// plain `mount`/`umount` act on whatever mount namespace the calling
    /// thread belongs to, which -- since this crate never calls
    /// `unshare(CLONE_NEWNS)` -- is always the shared host mount namespace.
    pub fn delete(name: &str) -> Res<()> {
        let path = Path::new(NETNS_PATH).join(name);
        if !path.exists() {
            return Err(InfraError::NamespaceNotFound(name.to_owned()));
        }

        umount(&path).map_err(|e| InfraError::Unmount {
            target: path.display().to_string(),
            source: e,
        })?;

        std::fs::remove_file(&path).map_err(|e| InfraError::Io {
            context: format!("remove {}", path.display()),
            source: e,
        })?;

        Ok(())
    }

    /// List the names of all namespaces currently registered under
    /// `/run/netns`.
    pub fn list() -> Result<Vec<String>, InfraError> {
        let entries = std::fs::read_dir(NETNS_PATH).map_err(|e| InfraError::Io {
            context: format!("read_dir {NETNS_PATH}"),
            source: e,
        })?;

        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| InfraError::Io {
                context: format!("read_dir entry in {NETNS_PATH}"),
                source: e,
            })?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name == ".lock" {
                continue;
            }
            names.push(name.into_owned());
        }
        Ok(names)
    }

    /// Execute an async closure **inside this namespace** on a dedicated,
    /// single-use OS thread, then return to the original namespace.
    ///
    /// # Namespace safety
    ///
    /// `setns(2)` is thread-local: it only affects the calling OS thread.
    /// We therefore need the closure to run pinned to a single OS thread
    /// for its entire duration.
    ///
    /// This deliberately uses a **dedicated `std::thread`**, not
    /// `tokio::task::spawn_blocking`. `spawn_blocking` pulls from a pool
    /// shared by the whole process; if this thread ever failed to restore
    /// its original namespace (see below), it would go back into that pool
    /// still sitting in the wrong namespace, and some *unrelated* piece of
    /// blocking work elsewhere in the process could later land on it and
    /// silently run its syscalls against the wrong namespace. A one-shot
    /// thread that is never reused eliminates that blast radius entirely:
    /// worst case, only this thread is affected, and it is about to exit
    /// anyway.
    ///
    /// A fresh single-threaded Tokio runtime is built inside that thread so
    /// that `rtnetlink`'s async machinery can run without ever migrating to
    /// another OS thread.
    ///
    /// If `f` panics, the panic is caught just long enough to attempt
    /// restoring the namespace, then re-raised (so it still propagates as a
    /// thread panic, surfaced to the caller as [`InfraError::ThreadPanicked`]).
    /// If restoring the namespace fails and `f` did *not* panic, that
    /// failure is returned as [`InfraError::RestoreNamespace`] rather than
    /// silently ignored.
    pub async fn run<F, Fut, T>(self, f: F) -> Result<T, InfraError>
    where
        F: FnOnce(rtnetlink::Handle) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, InfraError>> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let thread_label = self.path.display().to_string();

        std::thread::Builder::new()
            .name(format!("netns-run-{thread_label}"))
            .spawn(move || {
                let outcome = Self::run_on_dedicated_thread(self, f);
                // If the receiver was dropped (e.g. the awaiting future was
                // cancelled), there's nothing left to report to; ignore.
                let _ = tx.send(outcome);
            })
            .map_err(InfraError::SpawnThread)?;

        rx.await.map_err(|_| InfraError::ThreadPanicked)?
    }

    /// Runs entirely on the calling OS thread: saves the namespace, enters
    /// the target one, executes `f` on a private single-threaded runtime,
    /// then restores the original namespace before returning. Must be
    /// called from a thread dedicated to this single operation.
    fn run_on_dedicated_thread<F, Fut, T>(self, f: F) -> Result<T, InfraError>
    where
        F: FnOnce(rtnetlink::Handle) -> Fut,
        Fut: Future<Output = Result<T, InfraError>>,
    {
        // 1. Save the current thread's network namespace.
        let current = File::open(THREAD_SELF_NS_PATH).map_err(|e| InfraError::OpenNamespace {
            name: THREAD_SELF_NS_PATH.to_owned(),
            source: e,
        })?;

        // 2. Enter the target namespace.
        nix::sched::setns(self.fd.as_fd(), CloneFlags::CLONE_NEWNET).map_err(|e| {
            InfraError::SetNs {
                path: self.path.display().to_string(),
                source: e,
            }
        })?;

        // 3. Single-threaded runtime pinned to this OS thread.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(InfraError::Runtime)?;
        let _enter_guard = rt.enter();

        let (conn, handle, _) = rtnetlink::new_connection().map_err(InfraError::Runtime)?;
        rt.spawn(conn);

        // 4. Run `f`, catching panics just long enough to still attempt restoring the namespace
        //    before re-raising.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.block_on(f(handle))));

        // 5. Always attempt to restore, whether `f` panicked or not.
        let restore_result = nix::sched::setns(current.as_fd(), CloneFlags::CLONE_NEWNET);

        match result {
            Ok(inner) => {
                if let Err(e) = restore_result {
                    return Err(InfraError::RestoreNamespace {
                        path: THREAD_SELF_NS_PATH.to_owned(),
                        source: e,
                    });
                }
                inner
            }
            Err(panic_payload) => std::panic::resume_unwind(panic_payload),
        }
    }
}

/// Ensure `/run/netns` exists and is bind-mounted onto itself, marked
/// `MS_PRIVATE` so per-namespace bind mounts created under it don't
/// propagate to/from other mount namespaces. Mirrors what `ip netns`
/// does on first use. Idempotent, and safe across concurrent processes via
/// an flock on a lock file inside the directory.
fn ensure_netns_dir() -> Res<()> {
    let _in_process_guard = NETNS_DIR_BOOTSTRAP.lock().unwrap();

    std::fs::create_dir_all(NETNS_PATH).map_err(|e| InfraError::Io {
        context: format!("create {NETNS_PATH}"),
        source: e,
    })?;

    let lock_path = Path::new(NETNS_PATH).join(".lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| InfraError::Io {
            context: format!("open {}", lock_path.display()),
            source: e,
        })?;

    // Cross-process serialization for the check-then-mount below. The
    // returned guard holds the lock for the rest of this function's scope
    // and releases it automatically on drop.
    let _lock_guard = Flock::lock(lock_file, FlockArg::LockExclusive)
        .map_err(|(_file, e)| InfraError::Flock(e))?;

    if !is_mountpoint(NETNS_PATH)? {
        mount(
            Some(NETNS_PATH),
            NETNS_PATH,
            None::<&str>,
            MsFlags::MS_BIND,
            None::<&str>,
        )
        .map_err(|e| InfraError::Mount {
            target: NETNS_PATH.to_owned(),
            source: e,
        })?;

        mount(
            None::<&str>,
            NETNS_PATH,
            None::<&str>,
            MsFlags::MS_PRIVATE,
            None::<&str>,
        )
        .map_err(|e| InfraError::Mount {
            target: NETNS_PATH.to_owned(),
            source: e,
        })?;
    }

    // Lock released implicitly when `_lock_guard` drops at the end of this
    // scope.
    Ok(())
}

/// Whether `path` is a mount point, by comparing device IDs against its
/// parent directory (the standard trick: a mount point sits on a different
/// device than its parent, from the perspective of `stat(2)`).
fn is_mountpoint(path: &str) -> Result<bool, InfraError> {
    let canonical = std::fs::canonicalize(path).map_err(|e| InfraError::Io {
        context: format!("canonicalize {path}"),
        source: e,
    })?;

    let mountinfo =
        std::fs::read_to_string("/proc/self/mountinfo").map_err(|e| InfraError::Io {
            context: "read /proc/self/mountinfo".to_owned(),
            source: e,
        })?;

    // Field 5 (0-indexed 4) of each mountinfo line is the mount point.
    // See proc(5) for the full format.
    Ok(mountinfo.lines().any(|line| {
        line.split_whitespace()
            .nth(4)
            .map(Path::new)
            .is_some_and(|mount_point| mount_point == canonical)
    }))
}

/// Runs on a dedicated thread: `unshare`s a new net namespace (thread-local,
/// same constraint as `set_ns`), bind-mounts it onto `path` so it persists
/// independently of this thread, then brings `lo` up before the thread
/// exits.
fn create_netns_on_thread(path: &Path) -> Res<()> {
    unshare(CloneFlags::CLONE_NEWNET).map_err(InfraError::CreateNamespace)?;

    mount(
        Some(THREAD_SELF_NS_PATH),
        path,
        None::<&str>,
        MsFlags::MS_BIND,
        None::<&str>,
    )
    .map_err(|e| InfraError::Mount {
        target: path.display().to_string(),
        source: e,
    })?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(InfraError::Runtime)?;
    let _enter_guard = rt.enter();
    let (conn, handle, _) = rtnetlink::new_connection().map_err(InfraError::Runtime)?;
    rt.spawn(conn);

    rt.block_on(bring_up_loopback(&handle))
}

async fn bring_up_loopback(handle: &rtnetlink::Handle) -> Res<()> {
    let lo_index = handle
        .link()
        .get()
        .match_name("lo".to_owned())
        .execute()
        .try_next()
        .await
        .map_err(InfraError::Netlink)?
        .map(|msg| msg.header.index)
        .ok_or_else(|| InfraError::LinkNotFound("lo".to_owned()))?;

    let mut msg = LinkMessage::default();
    msg.header.index = lo_index;
    msg.header.flags = LinkFlags::Up;
    msg.header.change_mask = LinkFlags::Up;
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Best-effort rollback of a partially-created namespace file: the bind
/// mount may or may not have been established yet, so we try both, and
/// swallow errors from whichever step didn't apply.
fn rollback_netns_file(path: &Path) {
    let _ = umount(path);
    let _ = std::fs::remove_file(path);
}
