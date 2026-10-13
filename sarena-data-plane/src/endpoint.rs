use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
    EventQueue, Res,
    endpoint_events::{EP_CONFIG_NAME, EndpointEvent, EndpointWorker},
    id_allocator::EndpointId,
};

const EVENT_QUEUE_CAPACITY: usize = 32;

/// The part of an endpoint that is persisted to disk.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointState {
    pub id: EndpointId,
}

impl std::str::FromStr for EndpointState {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
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

/// Read-only snapshot of an endpoint, published by its worker after each
/// change.
#[derive(Debug, Clone, PartialEq)]
pub struct EndpointStatus {
    pub state: EndpointState,
    pub phase: EndpointPhase,
    /// Number of successful regenerations.
    pub generation: u64,
}

pub struct Endpoint {
    id: EndpointId,
    event_queue: EventQueue<EndpointWorker>,
    status: watch::Receiver<EndpointStatus>,
}

impl Endpoint {
    /// Creates an endpoint whose config is persisted under `state_dir`.
    pub fn new(state: EndpointState, state_dir: impl Into<PathBuf>) -> Self {
        let id = state.id;
        let (worker, status) = EndpointWorker::new(state, state_dir.into());
        Self {
            id,
            event_queue: EventQueue::new(format!("endpoint-{id}"), EVENT_QUEUE_CAPACITY, worker),
            status,
        }
    }

    /// Recreates an endpoint from the config persisted under `state_dir`.
    pub fn restore(state_dir: impl Into<PathBuf>) -> Res<Self> {
        let state_dir = state_dir.into();
        let config = std::fs::read_to_string(state_dir.join(EP_CONFIG_NAME))?.parse()?;
        Ok(Self::new(config, state_dir))
    }

    pub fn id(&self) -> EndpointId {
        self.id
    }

    /// The latest published status. Immediate: reads the snapshot, no event.
    pub fn status(&self) -> EndpointStatus {
        self.status.borrow().clone()
    }

    /// A receiver to wait for status changes, e.g. `changed().await`.
    pub fn watch_status(&self) -> watch::Receiver<EndpointStatus> {
        self.status.clone()
    }

    /// Queues a regeneration and waits until it has run.
    pub async fn regenerate(&self) -> Res<()> {
        self.submit(EndpointEvent::Regenerate).await
    }

    async fn submit(&self, event: EndpointEvent) -> Res<()> {
        self.event_queue.enqueue(event).await?.await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn regenerate_publishes_status_and_persists_config() {
        let dir = std::env::temp_dir().join(format!("sarena-ep-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let state: EndpointState = r#"{"id":42}"#.parse().unwrap();
        let ep = Endpoint::new(state.clone(), &dir);
        assert_eq!(ep.status().phase, EndpointPhase::Created);

        let mut status = ep.watch_status();
        ep.regenerate().await.unwrap();
        ep.regenerate().await.unwrap();

        assert!(status.has_changed().unwrap());
        let now = status.borrow_and_update().clone();
        assert_eq!(now.phase, EndpointPhase::Ready);
        assert_eq!(now.generation, 2);
        assert_eq!(ep.status(), now);

        let restored = Endpoint::restore(&dir).unwrap();
        assert_eq!(restored.id(), ep.id());
        assert_eq!(restored.status().state, state);
        assert_eq!(restored.status().phase, EndpointPhase::Created);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
