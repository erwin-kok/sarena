use std::{fs::OpenOptions, io::Write, path::Path};

use serde::{Deserialize, Serialize};

use crate::{DataPlaneError, EventQueue, Res, id_allocator::EndpointId};

const EP_CONFIG_NAME: &str = "ep_config.json";
const EVENT_QUEUE_CAPACITY: usize = 32;

pub struct Endpoint {
    id: Option<EndpointId>,

    event_queue: EventQueue,
}

impl Endpoint {
    pub fn id(&self) -> Option<EndpointId> {
        self.id
    }

    pub fn parse_endpoint(ep_str: &str) -> Res<Self> {
        let serializable: SerializableEndpoint = ep_str.parse()?;
        let Some(id) = serializable.id else {
            return Err(DataPlaneError::GeneralError(
                "Endpoint did not have an id".to_string(),
            ));
        };
        Ok(Self {
            id: Some(id),
            event_queue: EventQueue::new(format!("endpoint-{id}"), EVENT_QUEUE_CAPACITY),
        })
    }

    pub fn write_endpoint(&self, prefix: &Path) -> Res<()> {
        let path = prefix.join(EP_CONFIG_NAME);
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let serializable = SerializableEndpoint::from(self);
        let json = serde_json::to_string(&serializable)?;
        f.write_all(json.as_bytes())?;
        Ok(())
    }
}

impl From<&Endpoint> for SerializableEndpoint {
    fn from(endpoint: &Endpoint) -> Self {
        Self { id: endpoint.id }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct SerializableEndpoint {
    id: Option<EndpointId>,
}

impl std::str::FromStr for SerializableEndpoint {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}
