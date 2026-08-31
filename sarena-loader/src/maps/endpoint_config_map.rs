use std::path::Path;

use aya::maps::{Array, Map, MapData, MapError};
use sarena_shared::EndpointConfig;

use crate::{
    error::{LoaderError, Res},
    maps::EndpointMap,
    pin::PinRoot,
};

/// Per-endpoint singleton config array (`Array<EndpointConfig>`, index 0).
pub const ENDPOINT_CONFIG_MAP_NAME: &str = "endpoint_config";

pub struct EndpointConfigMap {
    map: Array<MapData, EndpointConfig>,
}

impl std::fmt::Debug for EndpointConfigMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointConfigMap").finish_non_exhaustive()
    }
}

impl EndpointConfigMap {
    /// Open the `endpoint_config` map for `link` from its pin. Errors if the
    /// endpoint has not been loaded (the pin does not exist).
    pub fn for_link(pins: &PinRoot, link: &str) -> Res<Self> {
        Self::from_pin(&pins.per_endpoint_map_dir(EndpointMap::EndpointConfigMap, link))
    }

    fn from_pin(path: &Path) -> Res<Self> {
        let data = MapData::from_pin(path).map_err(|e| LoaderError::MapOpen {
            path: path.to_path_buf(),
            src: e.to_string(),
        })?;
        let map = Map::from_map_data(data).map_err(|e| access(&e))?;
        let map = Array::try_from(map).map_err(|e| access(&e))?;
        Ok(Self { map })
    }

    /// Write this endpoint's configuration.
    pub fn set(&mut self, config: EndpointConfig) -> Res<()> {
        self.map.set(0, &config, 0).map_err(|e| access(&e))
    }

    /// Read this endpoint's configuration back.
    pub fn get(&self) -> Res<EndpointConfig> {
        self.map.get(&0, 0).map_err(|e| access(&e))
    }
}

fn access(e: &MapError) -> LoaderError {
    LoaderError::MapAccess {
        map: ENDPOINT_CONFIG_MAP_NAME,
        src: e.to_string(),
    }
}
