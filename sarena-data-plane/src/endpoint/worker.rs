use std::{
    fs::{self, File},
    io::Write,
    net::{Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::EndpointId;
use crate::{DataPlaneError, Res};

pub(crate) const EP_CONFIG_NAME: &str = "ep_config.json";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointPersist {
    #[serde(rename = "id")]
    pub id: EndpointId,

    #[serde(rename = "container_name", skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,

    #[serde(rename = "container_id", skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,

    #[serde(rename = "docker_endpoint", skip_serializing_if = "Option::is_none")]
    pub docker_endpoint: Option<String>,

    #[serde(rename = "ipv4", skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,

    #[serde(rename = "ipv6", skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<Ipv6Addr>,

    #[serde(rename = "k8s_ns", skip_serializing_if = "Option::is_none")]
    pub k8s_ns: Option<String>,

    #[serde(rename = "k8s_pod_name", skip_serializing_if = "Option::is_none")]
    pub k8s_pod_name: Option<String>,
}

impl EndpointPersist {
    pub fn read_state(state_dir: impl Into<PathBuf>) -> Res<Self> {
        let state_dir = state_dir.into();
        let content = std::fs::read_to_string(state_dir.join(EP_CONFIG_NAME))?;
        let state = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub fn write_state(&self, state_dir: &Path) -> Res<()> {
        let path = state_dir.join(EP_CONFIG_NAME);
        let tmp = state_dir.join(format!("{EP_CONFIG_NAME}.tmp"));
        let json = serde_json::to_string(self)?;
        let mut f = File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, &path)?;
        // The rename lives in the directory entry; sync it too, so it survives
        // a crash.
        File::open(state_dir)?.sync_all()?;
        Ok(())
    }

    pub fn k8s_ns_and_pod_name(&self) -> Option<String> {
        Some(format!(
            "{}/{}",
            self.k8s_ns.as_deref()?,
            self.k8s_pod_name.as_deref()?
        ))
    }
}

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
    /// Number of successful regenerations.
    pub generation: u64,
}

/// Owns the endpoint's mutable state and handles its commands one at a time,
/// in the order received. Lives inside its own task, so every command gets
/// `&mut self` without locking; [`Endpoint`](crate::endpoint::Endpoint) is
/// the handle to it.
pub(crate) struct EndpointWorker {
    state: EndpointState,
    state_dir: PathBuf,
    command_rx: mpsc::Receiver<EndpointCommand>,
    status_tx: watch::Sender<EndpointState>,
    stop: CancellationToken,
}

impl EndpointWorker {
    pub(crate) fn new(
        state: EndpointState,
        state_dir: PathBuf,
        command_rx: mpsc::Receiver<EndpointCommand>,
        status_tx: watch::Sender<EndpointState>,
        stop: CancellationToken,
    ) -> Self {
        Self {
            state,
            state_dir,
            command_rx,
            status_tx,
            stop,
        }
    }

    /// Moves the worker into its own task. The task exits once `stop` is
    /// cancelled and the queue is drained, or once every command sender is
    /// dropped.
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

    fn publish(&self) {
        let snapshot = self.state.clone();
        self.status_tx.send_replace(snapshot);
    }

    async fn regenerate(&mut self) -> Res<()> {
        self.state.phase = EndpointPhase::Regenerating;
        self.publish();

        // TODO: the actual regeneration work for `self.state.persist.id`.
        let result = self.persist().await;

        match &result {
            Ok(()) => {
                self.state.phase = EndpointPhase::Ready;
                self.state.generation += 1;
            }
            Err(e) => {
                tracing::warn!("regeneration failed: {e}");
                self.state.phase = EndpointPhase::Failed(e.to_string());
            }
        }
        self.publish();
        result
    }

    /// Writes the config to disk. Only the worker calls this, so writes never
    /// race and the file always matches the worker's state.
    async fn persist(&self) -> Res<()> {
        let persist = self.state.persist.clone();
        let state_dir = self.state_dir.clone();
        tokio::task::spawn_blocking(move || persist.write_state(&state_dir))
            .await
            .map_err(|e| DataPlaneError::GeneralError(format!("persist task failed: {e}")))?
    }
}
