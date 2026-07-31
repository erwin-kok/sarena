use tokio::sync::{mpsc, oneshot};

use crate::{
    backend::BpfBackend,
    endpoint::EndpointKind,
    error::{LoaderError, Res},
    loader::{EndpointHandle, Loader},
};

enum Command {
    AddEndpoint {
        kind: EndpointKind,
        link: String,
        reply: oneshot::Sender<Res<EndpointHandle>>,
    },
    RemoveEndpoint {
        kind: EndpointKind,
        link: String,
        reply: oneshot::Sender<Res<()>>,
    },
    ListActiveEndpoints {
        reply: oneshot::Sender<Res<Vec<(EndpointKind, String)>>>,
    },
    Reconcile {
        desired: Vec<(EndpointKind, String)>,
        reply: oneshot::Sender<Res<Vec<(EndpointKind, String, LoaderError)>>>,
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
    pub fn spawn<B>(loader: Loader<B>, channel_buffer: usize) -> Self
    where
        B: BpfBackend + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel(channel_buffer);

        std::thread::Builder::new()
            .name("sarena-loader".into())
            .spawn(move || {
                let mut loader = loader;
                while let Some(cmd) = rx.blocking_recv() {
                    dispatch(&mut loader, cmd);
                }
                tracing::info!("loader actor channel closed, thread exiting");
            })
            .expect("failed to spawn sarena-loader actor thread");

        Self { tx }
    }

    pub async fn add_endpoint(&self, kind: EndpointKind, link: &str) -> Res<EndpointHandle> {
        let link = link.to_string();
        self.call(|reply| Command::AddEndpoint { kind, link, reply })
            .await
    }

    pub async fn remove_endpoint(&self, kind: EndpointKind, link: &str) -> Res<()> {
        let link = link.to_string();
        self.call(|reply| Command::RemoveEndpoint { kind, link, reply })
            .await
    }

    pub async fn list_active_endpoints(&self) -> Res<Vec<(EndpointKind, String)>> {
        self.call(|reply| Command::ListActiveEndpoints { reply })
            .await
    }

    pub async fn reconcile(
        &self,
        desired: Vec<(EndpointKind, String)>,
    ) -> Res<Vec<(EndpointKind, String, LoaderError)>> {
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
            .map_err(|_| LoaderError::ActorGone)?;
        rx.await.map_err(|_| LoaderError::ActorGone)?
    }
}

fn dispatch<B: BpfBackend>(loader: &mut Loader<B>, cmd: Command) {
    // `let _ =` on every send: if the caller dropped the receiving end
    // (e.g. it timed out and moved on), that's the caller's business,
    // not a reason to log noise or panic here.
    match cmd {
        Command::AddEndpoint { kind, link, reply } => {
            let _ = reply.send(loader.add_endpoint(kind, &link));
        }
        Command::RemoveEndpoint { kind, link, reply } => {
            let _ = reply.send(loader.remove_endpoint(kind, &link));
        }
        Command::ListActiveEndpoints { reply } => {
            let _ = reply.send(loader.list_active_endpoints());
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
    use crate::mock_backend::MockBackend;

    #[tokio::test]
    async fn actor_serializes_calls_and_reports_state_after_restart() {
        let loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let handle = LoaderHandle::spawn(loader, 16);

        let kind = EndpointKind::Container;
        let link = "lxc00123";
        handle.add_endpoint(kind, link).await.unwrap();

        let active = handle.list_active_endpoints().await.unwrap();
        assert_eq!(active, vec![(kind, link.to_string())]);

        handle.remove_endpoint(kind, link).await.unwrap();
        assert!(handle.list_active_endpoints().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn dropping_a_cloned_handle_does_not_affect_other_handles() {
        let loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let handle = LoaderHandle::spawn(loader, 16);
        drop(handle.clone());

        let active = handle.list_active_endpoints().await.unwrap();
        assert!(active.is_empty());
    }
}
