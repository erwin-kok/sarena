use std::{path::PathBuf, sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::AbortHandle,
};
use tokio_util::sync::CancellationToken;

use super::{
    EndpointId,
    worker::{EndpointCommand, EndpointPhase, EndpointState, EndpointWorker},
};
use crate::{DataPlaneError, DataplaneLoader, EndpointPersist, Res};

const EVENT_QUEUE_CAPACITY: usize = 32;

#[derive(Clone)]
pub struct Endpoint {
    id: EndpointId,
    is_host: bool,
    command_tx: mpsc::Sender<EndpointCommand>,
    status_rx: watch::Receiver<EndpointState>,
    stop: CancellationToken,
    abort: AbortHandle,
}

impl Endpoint {
    pub fn new(
        loader: Arc<dyn DataplaneLoader>,
        state_dir: impl Into<PathBuf>,
        persist: &EndpointPersist,
    ) -> Self {
        let state = EndpointState {
            persist: persist.clone(),
            phase: EndpointPhase::Created,
            generation: 0,
        };
        let (command_tx, command_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (status_tx, status_rx) = watch::channel(state.clone());
        let stop = CancellationToken::new();
        let abort = EndpointWorker::new(
            loader,
            state,
            state_dir.into(),
            command_rx,
            status_tx,
            stop.clone(),
        )
        .spawn()
        .abort_handle();
        Self {
            id: persist.id,
            is_host: false,
            command_tx,
            status_rx,
            stop,
            abort,
        }
    }

    pub fn id(&self) -> EndpointId {
        self.id
    }

    pub fn is_host(&self) -> bool {
        self.is_host
    }

    /// The latest published status. Immediate: reads the snapshot, no event.
    pub fn status(&self) -> EndpointState {
        self.status_rx.borrow().clone()
    }

    /// A receiver to wait for status changes, e.g. `changed().await`.
    pub fn watch_status(&self) -> watch::Receiver<EndpointState> {
        self.status_rx.clone()
    }

    /// Queues a regeneration and waits until it has run.
    pub async fn regenerate(&self) -> Res<()> {
        self.call(|reply| EndpointCommand::Regenerate { reply })
            .await
    }

    /// Tells the worker to stop: new commands are refused, the ones already
    /// queued still run. Returns immediately, so a caller can stop many
    /// endpoints before waiting on any of them.
    pub(crate) fn shutdown(&self) {
        self.stop.cancel();
    }

    /// Waits for the worker to drain its queue and exit. The worker's exit is
    /// seen as its status sender going away. Dropping this handle's command
    /// sender means the worker also exits without [`shutdown`](Self::shutdown)
    /// if no other clone is left. If that takes longer than `timeout`, the
    /// worker is aborted: the running command is interrupted, the remaining
    /// ones are cancelled, and this returns an error.
    pub(crate) async fn wait_for_shutdown(self, timeout: Duration) -> Res<()> {
        let Self {
            id,
            command_tx,
            mut status_rx,
            abort,
            ..
        } = self;
        drop(command_tx);
        let exited = async { while status_rx.changed().await.is_ok() {} };
        if tokio::time::timeout(timeout, exited).await.is_ok() {
            Ok(())
        } else {
            abort.abort();
            Err(DataPlaneError::GeneralError(format!(
                "endpoint {} did not drain within {timeout:?}, aborted",
                u64::from(id)
            )))
        }
    }

    async fn call<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Res<T>>) -> EndpointCommand,
    ) -> Res<T> {
        let (reply, rx) = oneshot::channel();
        self.command_tx
            .send(build(reply))
            .await
            .map_err(|_| DataPlaneError::ActorGone)?;
        rx.await.map_err(|_| DataPlaneError::ActorCancelled)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EndpointSpec, MockDataplaneLoader, endpoint::directory::state_directory_path};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sarena-ep-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn loader() -> Arc<dyn DataplaneLoader> {
        Arc::new(MockDataplaneLoader::default())
    }

    #[tokio::test]
    async fn regenerate_publishes_status_and_persists_config() {
        let dir = temp_dir("regen");

        let state = EndpointPersist {
            id: EndpointId::new(42),
            spec: EndpointSpec::default(),
        };
        let ep = Endpoint::new(loader(), &dir, &state);
        assert_eq!(ep.status().phase, EndpointPhase::Created);

        let mut status = ep.watch_status();
        ep.regenerate().await.unwrap();
        ep.regenerate().await.unwrap();

        assert!(status.has_changed().unwrap());
        let now = status.borrow_and_update().clone();
        assert_eq!(now.phase, EndpointPhase::Ready);
        assert_eq!(now.generation, 2);
        assert_eq!(ep.status(), now);

        let persisted = EndpointPersist::read_state(state_directory_path(&dir, ep.id())).unwrap();
        let restored = Endpoint::new(loader(), &dir, &persisted);
        assert_eq!(restored.id(), ep.id());
        assert_eq!(restored.status().persist, state);
        assert_eq!(restored.status().phase, EndpointPhase::Created);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn shutdown_runs_commands_queued_before_stop() {
        let dir = temp_dir("drain");
        let state = EndpointState {
            persist: EndpointPersist {
                id: EndpointId::new(7),
                spec: EndpointSpec::default(),
            },
            phase: EndpointPhase::Created,
            generation: 0,
        };
        let (command_tx, command_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (status_tx, status_rx) = watch::channel(state.clone());

        let mut replies = Vec::new();
        for _ in 0..3 {
            let (reply, rx) = oneshot::channel();
            command_tx
                .try_send(EndpointCommand::Regenerate { reply })
                .unwrap();
            replies.push(rx);
        }
        // Stopped before the worker ever runs: everything queued must drain.
        let stop = CancellationToken::new();
        stop.cancel();
        EndpointWorker::new(loader(), state, dir.clone(), command_rx, status_tx, stop)
            .spawn()
            .await
            .unwrap();

        for rx in replies {
            rx.await.unwrap().unwrap();
        }
        assert_eq!(status_rx.borrow().generation, 3);
        assert!(command_tx.is_closed());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn wait_for_shutdown_without_shutdown_exits_once_sender_is_dropped() {
        let dir = temp_dir("drop");
        let ep = Endpoint::new(
            loader(),
            &dir,
            &EndpointPersist {
                id: EndpointId::new(8),
                spec: EndpointSpec::default(),
            },
        );
        ep.regenerate().await.unwrap();
        ep.wait_for_shutdown(Duration::from_secs(5)).await.unwrap();

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Paused time: if the worker never exited, the timeout would fire at once
    // instead of hanging the test.
    #[tokio::test(start_paused = true)]
    async fn wait_for_shutdown_returns_on_worker_exit_without_any_status_published() {
        let dir = temp_dir("quiet");
        let ep = Endpoint::new(
            loader(),
            &dir,
            &EndpointPersist {
                id: EndpointId::new(10),
                spec: EndpointSpec::default(),
            },
        );
        // A live clone keeps a command sender, so only `shutdown` can end the
        // worker, and only its status sender going away can end the wait.
        let clone = ep.clone();
        let status = ep.watch_status();

        ep.shutdown();
        ep.wait_for_shutdown(Duration::from_secs(60)).await.unwrap();

        // Closed, and still holding the initial snapshot: nothing was published.
        assert!(status.has_changed().is_err());
        assert_eq!(status.borrow().phase, EndpointPhase::Created);
        assert_eq!(status.borrow().generation, 0);
        assert!(matches!(
            clone.regenerate().await,
            Err(DataPlaneError::ActorGone)
        ));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_shutdown_without_shutdown_times_out_while_a_clone_is_alive() {
        let dir = temp_dir("clone");
        let ep = Endpoint::new(
            loader(),
            &dir,
            &EndpointPersist {
                id: EndpointId::new(11),
                spec: EndpointSpec::default(),
            },
        );
        let clone = ep.clone();

        assert!(ep.wait_for_shutdown(Duration::from_secs(1)).await.is_err());
        // Timed out, so the worker was aborted: the clone's commands fail.
        assert!(clone.regenerate().await.is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_shutdown_times_out_aborts_and_cancels_queued_commands() {
        let (command_tx, command_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (status_tx, status_rx) = watch::channel(EndpointState {
            persist: EndpointPersist {
                id: EndpointId::new(9),
                spec: EndpointSpec::default(),
            },
            phase: EndpointPhase::Created,
            generation: 0,
        });
        // A worker stuck in a command: it holds the queue but never drains it.
        let abort = tokio::spawn(async move {
            let _command_rx = command_rx;
            let _status_tx = status_tx;
            std::future::pending::<()>().await;
        })
        .abort_handle();
        let ep = Endpoint {
            id: EndpointId::new(9),
            is_host: false,
            command_tx: command_tx.clone(),
            status_rx,
            stop: CancellationToken::new(),
            abort,
        };
        let (reply, rx) = oneshot::channel();
        command_tx
            .try_send(EndpointCommand::Regenerate { reply })
            .unwrap();

        ep.shutdown();
        assert!(ep.wait_for_shutdown(Duration::from_secs(1)).await.is_err());
        // The abort dropped the queue, and with it the reply sender.
        assert!(rx.await.is_err());
    }
}
