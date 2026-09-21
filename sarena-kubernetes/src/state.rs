use std::sync::Arc;

use kube::runtime::events::Reporter;
use serde::Serialize;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostics {
    #[serde(deserialize_with = "from_ts")]
    // pub last_event: DateTime<Utc>,
    #[serde(skip)]
    pub reporter: Reporter,
}

impl Diagnostics {
    pub fn new(component: String) -> Self {
        Self {
            // last_event: Utc::now(),
            reporter: component.into(),
        }
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            // last_event: Utc::now(),
            reporter: "sart".into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    pub registry: prometheus::Registry,
}
