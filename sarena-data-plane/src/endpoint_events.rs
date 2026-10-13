use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use tokio::sync::watch;

use crate::{
    DataPlaneError, Handler, Res,
    endpoint::{EndpointPhase, EndpointState, EndpointStatus},
};

pub(crate) const EP_CONFIG_NAME: &str = "ep_config.json";

/// The events an endpoint's queue handles, one at a time.
#[derive(Debug)]
pub(crate) enum EndpointEvent {
    Regenerate,
}

/// Owns the endpoint's mutable state. Lives inside the endpoint's event queue,
/// so every event gets `&mut self` without locking. Never shared, so it may
/// hold non-`Clone` resources; readers get an [`EndpointStatus`] snapshot.
pub(crate) struct EndpointWorker {
    config: EndpointState,
    state_dir: PathBuf,
    phase: EndpointPhase,
    generation: u64,
    status: watch::Sender<EndpointStatus>,
}

impl EndpointWorker {
    pub(crate) fn new(
        config: EndpointState,
        state_dir: PathBuf,
    ) -> (Self, watch::Receiver<EndpointStatus>) {
        let (status, rx) = watch::channel(EndpointStatus {
            state: config.clone(),
            phase: EndpointPhase::Created,
            generation: 0,
        });
        let worker = Self {
            config,
            state_dir,
            phase: EndpointPhase::Created,
            generation: 0,
            status,
        };
        (worker, rx)
    }

    fn snapshot(&self) -> EndpointStatus {
        EndpointStatus {
            state: self.config.clone(),
            phase: self.phase.clone(),
            generation: self.generation,
        }
    }

    fn publish(&self) {
        self.status.send_replace(self.snapshot());
    }

    async fn regenerate(&mut self) -> Res<()> {
        self.phase = EndpointPhase::Regenerating;
        self.publish();

        // TODO: the actual regeneration work for `self.config.id`.
        let result = self.persist().await;

        match &result {
            Ok(()) => {
                self.phase = EndpointPhase::Ready;
                self.generation += 1;
            }
            Err(e) => self.phase = EndpointPhase::Failed(e.to_string()),
        }
        self.publish();
        result
    }

    /// Writes the config to disk. Only the worker calls this, so writes never
    /// race and the file always matches the worker's state.
    async fn persist(&self) -> Res<()> {
        let config = self.config.clone();
        let dir = self.state_dir.clone();
        tokio::task::spawn_blocking(move || write_config(&config, &dir))
            .await
            .map_err(|e| DataPlaneError::GeneralError(format!("persist task failed: {e}")))?
    }
}

fn write_config(config: &EndpointState, dir: &Path) -> Res<()> {
    let path = dir.join(EP_CONFIG_NAME);
    let tmp = dir.join(format!("{EP_CONFIG_NAME}.tmp"));
    let json = serde_json::to_string(config)?;
    let mut f = File::create(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

impl Handler for EndpointWorker {
    type Event = EndpointEvent;
    type Output = Res<()>;

    async fn handle(&mut self, event: EndpointEvent) -> Res<()> {
        match event {
            EndpointEvent::Regenerate => self.regenerate().await,
        }
    }

    fn event_name(event: &EndpointEvent) -> &'static str {
        match event {
            EndpointEvent::Regenerate => "regenerate",
        }
    }
}
