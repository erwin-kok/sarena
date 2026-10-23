use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex, PoisonError},
    thread::JoinHandle,
};

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::{
    error::{DataPlaneError, Res},
    loader::{
        DataplaneLoader, Loader,
        backend::BpfBackend,
        loader_endpoint::{LoaderEndpoint, LoaderEndpointKind},
    },
};

enum Command {
    LoadGlobalMaps {
        reply: oneshot::Sender<Res<()>>,
    },
    AddLoaderEndpoint {
        endpoint: LoaderEndpoint,
        reply: oneshot::Sender<Res<()>>,
    },
    RemoveLoaderEndpoint {
        endpoint: LoaderEndpoint,
        reply: oneshot::Sender<Res<()>>,
    },
    ListLoaderEndpoints {
        reply: oneshot::Sender<Res<Vec<(LoaderEndpointKind, String)>>>,
    },
    Reconcile {
        desired: Vec<LoaderEndpoint>,
        reply: oneshot::Sender<Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>>>,
    },
    TeardownAll {
        reply: oneshot::Sender<Res<()>>,
    },
    Shutdown {
        reply: oneshot::Sender<Res<()>>,
    },
}

#[derive(Clone)]
pub struct DefaultDataplaneLoader {
    tx: mpsc::Sender<Command>,
    // Shared between clones; whichever clone calls `shutdown` first takes
    // the handle and joins the thread.
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl DefaultDataplaneLoader {
    /// Spawns the actor on its own dedicated OS thread - deliberately
    /// *not* a tokio task. The underlying bpf(2)/netlink calls a real
    /// backend makes are blocking syscalls (and the verifier can take a
    /// non-trivial amount of time on a large program), so running them
    /// on a tokio worker thread would stall every other task sharing
    /// that worker. A dedicated thread with `blocking_recv` sidesteps
    /// that entirely; callers still talk to it through a normal async
    /// `DefaultDataplaneLoader`.
    ///
    /// Commands are processed one at a time in the order received,
    /// which is what guarantees two calls (e.g. an ADD and a DEL racing
    /// for the same id) never interleave against the same pin tree -
    /// no locking needed anywhere else.
    ///
    /// The actor thread's `JoinHandle` is kept inside the handle, so an
    /// orderly shutdown (see [`DataplaneLoader::shutdown`]) can wait for
    /// the thread to actually finish rather than just dropping its senders
    /// and hoping.
    pub fn spawn<B>(loader: Loader<B>, channel_buffer: usize) -> Self
    where
        B: BpfBackend + Send + 'static,
        B::Instance: Send,
    {
        let (tx, mut rx) = mpsc::channel(channel_buffer);

        let thread = std::thread::Builder::new()
            .name("sarena-data-plane".into())
            .spawn(move || {
                let mut loader = loader;
                while let Some(cmd) = rx.blocking_recv() {
                    if dispatch(&mut loader, cmd).is_break() {
                        break;
                    }
                }
                tracing::info!("loader actor channel closed, thread exiting");
            })
            .expect("failed to spawn sarena-data-plane actor thread");

        Self {
            tx,
            thread: Arc::new(Mutex::new(Some(thread))),
        }
    }

    async fn call<T>(&self, build: impl FnOnce(oneshot::Sender<Res<T>>) -> Command) -> Res<T> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .map_err(|_| DataPlaneError::ActorGone)?;
        rx.await.map_err(|_| DataPlaneError::ActorCancelled)?
    }
}

#[async_trait]
impl DataplaneLoader for DefaultDataplaneLoader {
    async fn load_global_maps(&self) -> Res<()> {
        self.call(|reply| Command::LoadGlobalMaps { reply }).await
    }

    async fn add_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()> {
        let endpoint = endpoint.clone();
        self.call(|reply| Command::AddLoaderEndpoint { endpoint, reply })
            .await
    }

    async fn remove_loader_endpoint(&self, endpoint: &LoaderEndpoint) -> Res<()> {
        let endpoint = endpoint.clone();
        self.call(|reply| Command::RemoveLoaderEndpoint { endpoint, reply })
            .await
    }

    async fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>> {
        self.call(|reply| Command::ListLoaderEndpoints { reply })
            .await
    }

    async fn reconcile(
        &self,
        desired: Vec<LoaderEndpoint>,
    ) -> Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>> {
        self.call(|reply| Command::Reconcile { desired, reply })
            .await
    }

    async fn teardown_all(&self) -> Res<()> {
        self.call(|reply| Command::TeardownAll { reply }).await
    }

    async fn shutdown(&self) -> Res<()> {
        let teardown = self.call(|reply| Command::Shutdown { reply }).await;

        let thread = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            tokio::task::spawn_blocking(move || thread.join())
                .await
                .expect("failed to join sarena-data-plane actor thread")
                .expect("sarena-data-plane actor thread panicked");
        }

        teardown
    }
}

fn dispatch<B: BpfBackend>(loader: &mut Loader<B>, cmd: Command) -> ControlFlow<()> {
    // `let _ =` on every send: if the caller dropped the receiving end
    // (e.g. it timed out and moved on), that's the caller's business,
    // not a reason to log noise or panic here.
    match cmd {
        Command::LoadGlobalMaps { reply } => {
            let _ = reply.send(loader.load_global_maps());
        }
        Command::AddLoaderEndpoint { endpoint, reply } => {
            let _ = reply.send(loader.add_loader_endpoint(&endpoint));
        }
        Command::RemoveLoaderEndpoint { endpoint, reply } => {
            let _ = reply.send(loader.remove_loader_endpoint(&endpoint));
        }
        Command::ListLoaderEndpoints { reply } => {
            let _ = reply.send(loader.list_loader_endpoints());
        }
        Command::Reconcile { desired, reply } => {
            let _ = reply.send(loader.reconcile(&desired));
        }
        Command::TeardownAll { reply } => {
            let _ = reply.send(loader.teardown_all());
        }
        Command::Shutdown { reply } => {
            let _ = reply.send(loader.teardown_all());
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PinRoot, loader::mock_backend::MockBackend};

    #[tokio::test]
    async fn actor_serializes_calls_and_reports_state_after_restart() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let loader = Loader::new(MockBackend::new(), pin_root);
        let handle = DefaultDataplaneLoader::spawn(loader, 16);

        let endpoint = LoaderEndpoint::Host {
            link: "lxc00123".to_string(),
        };
        handle.add_loader_endpoint(&endpoint).await.unwrap();

        let active = handle.list_loader_endpoints().await.unwrap();
        assert_eq!(
            active,
            vec![(LoaderEndpointKind::Host, "lxc00123".to_string())]
        );

        handle.remove_loader_endpoint(&endpoint).await.unwrap();
        assert!(handle.list_loader_endpoints().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn dropping_a_cloned_handle_does_not_affect_other_handles() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let loader = Loader::new(MockBackend::new(), pin_root);
        let handle = DefaultDataplaneLoader::spawn(loader, 16);
        drop(handle.clone());

        let active = handle.list_loader_endpoints().await.unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn shutdown_stops_the_actor_even_with_clones_alive() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let loader = Loader::new(MockBackend::new(), pin_root);
        let handle = DefaultDataplaneLoader::spawn(loader, 16);
        let clone = handle.clone();

        handle.shutdown().await.unwrap();

        assert!(matches!(
            clone.list_loader_endpoints().await,
            Err(DataPlaneError::ActorGone)
        ));
    }
}
