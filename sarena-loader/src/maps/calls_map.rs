use std::path::Path;

use aya::maps::{Array, Map, MapData, MapError};

use crate::{
    error::{LoaderError, Res},
    maps::EndpointMap,
    pin::PinRoot,
};

/// Per-endpoint tail-call / scratch array (`Array<u32>`).
pub const CALLS_MAP_NAME: &str = "calls_map";

pub struct CallsMap {
    map: Array<MapData, u32>,
}

impl std::fmt::Debug for CallsMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallsMap").finish_non_exhaustive()
    }
}

impl CallsMap {
    pub fn for_link(pins: &PinRoot, link: &str) -> Res<Self> {
        Self::from_pin(&pins.per_endpoint_map_dir(EndpointMap::CallsMap, link))
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

    pub fn set(&mut self, index: u32, value: u32) -> Res<()> {
        self.map.set(index, &value, 0).map_err(|e| access(&e))
    }

    pub fn get(&self, index: u32) -> Res<u32> {
        self.map.get(&index, 0).map_err(|e| access(&e))
    }
}

fn access(e: &MapError) -> LoaderError {
    LoaderError::MapAccess {
        map: CALLS_MAP_NAME,
        src: e.to_string(),
    }
}
