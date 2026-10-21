use std::{path::PathBuf, sync::Arc};

use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::{DataplaneLoader, EndpointId, EndpointPersist, Res};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum EndpointPhase {
    #[default]
    Created,
    Regenerating,
    Ready,
    Failed(String),
}

#[derive(Debug)]
pub(crate) enum EndpointCommand {
    Regenerate { reply: oneshot::Sender<Res<()>> },
}

/// Read-only snapshot of an endpoint, published by its worker after each
/// change.
#[derive(Debug, Clone, PartialEq)]
pub struct EndpointState {
    pub persist: EndpointPersist,
    pub phase: EndpointPhase,
    pub generation: u64,
}

impl EndpointState {
    pub fn id(&self) -> EndpointId {
        self.persist.id
    }
}

/// Owns the endpoint's mutable state and handles its commands one at a time,
/// in the order received. Lives inside its own task, so every command gets
/// `&mut self` without locking; [`Endpoint`](crate::endpoint::Endpoint) is
/// the handle to it.
pub(crate) struct EndpointWorker {
    pub(crate) loader: Arc<dyn DataplaneLoader>,
    pub(crate) state: EndpointState,
    pub(crate) state_dir: PathBuf,
    command_rx: mpsc::Receiver<EndpointCommand>,
    status_tx: watch::Sender<EndpointState>,
    stop: CancellationToken,
}

impl EndpointWorker {
    pub(crate) fn new(
        loader: Arc<dyn DataplaneLoader>,
        state: EndpointState,
        state_dir: PathBuf,
        command_rx: mpsc::Receiver<EndpointCommand>,
        status_tx: watch::Sender<EndpointState>,
        stop: CancellationToken,
    ) -> Self {
        Self {
            loader,
            state,
            state_dir,
            command_rx,
            status_tx,
            stop,
        }
    }

    pub(crate) fn spawn(self) -> JoinHandle<()> {
        let span = tracing::info_span!("endpoint", id = u64::from(self.state.persist.id));
        tokio::spawn(self.run().instrument(span))
    }

    async fn run(mut self) {
        loop {
            let command = tokio::select! {
                biased;

                () = self.stop.cancelled() => break,

                command = self.command_rx.recv() => match command {
                    Some(command) => command,
                    // All senders are gone, so the queue is already empty.
                    None => return,
                },
            };
            self.handle(command).await;
        }

        // Drain: refuse new commands, but still run the ones already queued.
        self.command_rx.close();
        while let Some(command) = self.command_rx.recv().await {
            self.handle(command).await;
        }
        tracing::debug!("endpoint queue drained, worker exiting");
    }

    // `let _ =` on every reply: if the caller stopped waiting, the command
    // still ran; there is just nobody left to tell.
    async fn handle(&mut self, command: EndpointCommand) {
        match command {
            EndpointCommand::Regenerate { reply } => {
                let _ = reply.send(self.regenerate().await);
            }
        }
    }

    pub(crate) fn publish(&self) {
        let snapshot = self.state.clone();
        self.status_tx.send_replace(snapshot);
    }
}
