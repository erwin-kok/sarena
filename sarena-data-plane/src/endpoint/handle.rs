use std::{path::PathBuf, time::Duration};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use super::{
    EndpointId,
    worker::{EndpointCommand, EndpointPhase, EndpointState, EndpointWorker},
};
use crate::{DataPlaneError, EndpointPersist, Res};

const EVENT_QUEUE_CAPACITY: usize = 32;

pub struct Endpoint {
    id: EndpointId,
    is_host: bool,
    command_tx: mpsc::Sender<EndpointCommand>,
    status_rx: watch::Receiver<EndpointState>,
    stop: CancellationToken,
    join_handle: JoinHandle<()>,
}

impl Endpoint {
    pub fn new(persist: &EndpointPersist, state_dir: impl Into<PathBuf>) -> Self {
        let state = EndpointState {
            persist: persist.clone(),
            phase: EndpointPhase::Created,
            generation: 0,
        };
        let (command_tx, command_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (status_tx, status_rx) = watch::channel(state.clone());
        let stop = CancellationToken::new();
        let join_handle =
            EndpointWorker::new(state, state_dir.into(), command_rx, status_tx, stop.clone())
                .spawn();
        Self {
            id: persist.id,
            is_host: false,
            command_tx,
            status_rx,
            stop,
            join_handle,
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

    /// Tells the worker to stop: new commands are refused, the ones already
    /// queued still run. Returns immediately, so a caller can stop many
    /// endpoints before waiting on any of them.
    pub fn shutdown(&self) {
        self.stop.cancel();
    }

    /// Waits for the worker to drain its queue and exit. Dropping the command
    /// sender here means the worker exits even if [`shutdown`](Self::shutdown)
    /// was never called. If that takes longer than `timeout`, the worker is
    /// aborted: the running command is interrupted, the remaining ones are
    /// cancelled, and this returns an error.
    pub async fn wait_for_shutdown(self, timeout: Duration) -> Res<()> {
        let Self {
            id,
            command_tx,
            mut join_handle,
            ..
        } = self;
        drop(command_tx);
        if let Ok(joined) = tokio::time::timeout(timeout, &mut join_handle).await {
            joined.map_err(|e| DataPlaneError::GeneralError(format!("endpoint worker failed: {e}")))
        } else {
            join_handle.abort();
            Err(DataPlaneError::GeneralError(format!(
                "endpoint {} did not drain within {timeout:?}, aborted",
                u64::from(id)
            )))
        }
    }

    /// Queues a regeneration and waits until it has run.
    pub async fn regenerate(&self) -> Res<()> {
        self.call(|reply| EndpointCommand::Regenerate { reply })
            .await
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
    use crate::EndpointSpec;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sarena-ep-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn regenerate_publishes_status_and_persists_config() {
        let dir = temp_dir("regen");

        let state = EndpointPersist {
            id: EndpointId::new(42),
            spec: EndpointSpec::default(),
        };
        let ep = Endpoint::new(&state, &dir);
        assert_eq!(ep.status().phase, EndpointPhase::Created);

        let mut status = ep.watch_status();
        ep.regenerate().await.unwrap();
        ep.regenerate().await.unwrap();

        assert!(status.has_changed().unwrap());
        let now = status.borrow_and_update().clone();
        assert_eq!(now.phase, EndpointPhase::Ready);
        assert_eq!(now.generation, 2);
        assert_eq!(ep.status(), now);

        let restored = Endpoint::new(&EndpointPersist::read_state(&dir).unwrap(), &dir);
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
        EndpointWorker::new(state, dir.clone(), command_rx, status_tx, stop)
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
            &EndpointPersist {
                id: EndpointId::new(8),
                spec: EndpointSpec::default(),
            },
            &dir,
        );
        ep.regenerate().await.unwrap();
        ep.wait_for_shutdown(Duration::from_secs(5)).await.unwrap();

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn wait_for_shutdown_times_out_aborts_and_cancels_queued_commands() {
        let (command_tx, command_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let (_status_tx, status_rx) = watch::channel(EndpointState {
            persist: EndpointPersist {
                id: EndpointId::new(9),
                spec: EndpointSpec::default(),
            },
            phase: EndpointPhase::Created,
            generation: 0,
        });
        // A worker stuck in a command: it holds the queue but never drains it.
        let join_handle = tokio::spawn(async move {
            let _command_rx = command_rx;
            std::future::pending::<()>().await;
        });
        let ep = Endpoint {
            id: EndpointId::new(9),
            is_host: false,
            command_tx: command_tx.clone(),
            status_rx,
            stop: CancellationToken::new(),
            join_handle,
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
