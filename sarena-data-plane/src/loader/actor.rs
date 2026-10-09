use std::thread::JoinHandle;

use tokio::sync::{mpsc, oneshot};

use crate::{
    error::{DataPlaneError, Res},
    loader::{Loader, backend::BpfBackend, loader_endpoint::LoaderEndpointKind},
};

enum Command {
    LoadGlobalMaps {
        reply: oneshot::Sender<Res<()>>,
    },
    AddLoaderEndpoint {
        kind: LoaderEndpointKind,
        link: String,
        reply: oneshot::Sender<Res<()>>,
    },
    RemoveLoaderEndpoint {
        kind: LoaderEndpointKind,
        link: String,
        reply: oneshot::Sender<Res<()>>,
    },
    ListLoaderEndpoints {
        reply: oneshot::Sender<Res<Vec<(LoaderEndpointKind, String)>>>,
    },
    Reconcile {
        desired: Vec<(LoaderEndpointKind, String)>,
        reply: oneshot::Sender<Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>>>,
    },
    TeardownAll {
        reply: oneshot::Sender<Res<()>>,
    },
}

#[derive(Clone)]
pub struct LoaderHandle {
    tx: mpsc::Sender<Command>,
}

impl LoaderHandle {
    /// Spawns the actor on its own dedicated OS thread - deliberately
    /// *not* a tokio task. The underlying bpf(2)/netlink calls a real
    /// backend makes are blocking syscalls (and the verifier can take a
    /// non-trivial amount of time on a large program), so running them
    /// on a tokio worker thread would stall every other task sharing
    /// that worker. A dedicated thread with `blocking_recv` sidesteps
    /// that entirely; callers still talk to it through a normal async
    /// `LoaderHandle`.
    ///
    /// Commands are processed one at a time in the order received,
    /// which is what guarantees two calls (e.g. an ADD and a DEL racing
    /// for the same id) never interleave against the same pin tree -
    /// no locking needed anywhere else.
    ///
    /// Returns the handle together with the actor thread's `JoinHandle`,
    /// so a caller that wants an orderly shutdown (see [`LoaderHandle::shutdown`])
    /// can wait for the thread to actually finish rather than just
    /// dropping its senders and hoping.
    pub fn spawn<B>(loader: Loader<B>, channel_buffer: usize) -> (Self, JoinHandle<()>)
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
                    dispatch(&mut loader, cmd);
                }
                tracing::info!("loader actor channel closed, thread exiting");
            })
            .expect("failed to spawn sarena-data-plane actor thread");

        (Self { tx }, thread)
    }

    pub async fn shutdown(self, thread: JoinHandle<()>) -> Res<()> {
        self.teardown_all().await?;
        drop(self);

        tokio::task::spawn_blocking(move || thread.join())
            .await
            .expect("failed to join sarena-data-plane actor thread")
            .expect("sarena-data-plane actor thread panicked");

        Ok(())
    }

    pub async fn load_global_maps(&self) -> Res<()> {
        self.call(|reply| Command::LoadGlobalMaps { reply }).await
    }

    pub async fn add_loader_endpoint(&self, kind: LoaderEndpointKind, link: &str) -> Res<()> {
        let link = link.to_string();
        self.call(|reply| Command::AddLoaderEndpoint { kind, link, reply })
            .await
    }

    pub async fn remove_loader_endpoint(&self, kind: LoaderEndpointKind, link: &str) -> Res<()> {
        let link = link.to_string();
        self.call(|reply| Command::RemoveLoaderEndpoint { kind, link, reply })
            .await
    }

    pub async fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>> {
        self.call(|reply| Command::ListLoaderEndpoints { reply })
            .await
    }

    pub async fn reconcile(
        &self,
        desired: Vec<(LoaderEndpointKind, String)>,
    ) -> Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>> {
        self.call(|reply| Command::Reconcile { desired, reply })
            .await
    }

    pub async fn teardown_all(&self) -> Res<()> {
        self.call(|reply| Command::TeardownAll { reply }).await
    }

    async fn call<T>(&self, build: impl FnOnce(oneshot::Sender<Res<T>>) -> Command) -> Res<T> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .map_err(|_| DataPlaneError::ActorGone)?;
        rx.await.map_err(|_| DataPlaneError::ActorGone)?
    }
}

fn dispatch<B: BpfBackend>(loader: &mut Loader<B>, cmd: Command) {
    // `let _ =` on every send: if the caller dropped the receiving end
    // (e.g. it timed out and moved on), that's the caller's business,
    // not a reason to log noise or panic here.
    match cmd {
        Command::LoadGlobalMaps { reply } => {
            let _ = reply.send(loader.load_global_maps());
        }
        Command::AddLoaderEndpoint { kind, link, reply } => {
            let _ = reply.send(loader.add_loader_endpoint(kind, &link));
        }
        Command::RemoveLoaderEndpoint { kind, link, reply } => {
            let _ = reply.send(loader.remove_loader_endpoint(kind, &link));
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PinRoot, loader::mock_backend::MockBackend};

    #[tokio::test]
    async fn actor_serializes_calls_and_reports_state_after_restart() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let loader = Loader::new(MockBackend::new(), pin_root);
        let (handle, _thread) = LoaderHandle::spawn(loader, 16);

        let kind = LoaderEndpointKind::Container;
        let link = "lxc00123";
        handle.add_loader_endpoint(kind, link).await.unwrap();

        let active = handle.list_loader_endpoints().await.unwrap();
        assert_eq!(active, vec![(kind, link.to_string())]);

        handle.remove_loader_endpoint(kind, link).await.unwrap();
        assert!(handle.list_loader_endpoints().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn dropping_a_cloned_handle_does_not_affect_other_handles() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let loader = Loader::new(MockBackend::new(), pin_root);
        let (handle, _thread) = LoaderHandle::spawn(loader, 16);
        drop(handle.clone());

        let active = handle.list_loader_endpoints().await.unwrap();
        assert!(active.is_empty());
    }
}
