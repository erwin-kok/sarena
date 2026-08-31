use std::{net::Ipv4Addr, path::Path};

use aya::maps::{HashMap, Map, MapData, MapError};
use sarena_shared::{EndpointInfo, Ipv4Key, Ipv4KeyExt as _};

use crate::{
    error::{LoaderError, Res},
    maps::GlobalMap,
    pin::PinRoot,
};

/// Global endpoint lookup table: `Ipv4Key -> EndpointInfo`.
pub const LXC_MAP_NAME: &str = "lxc_map";

#[derive(Debug)]
pub struct LxcMap {
    map: HashMap<MapData, Ipv4Key, EndpointInfo>,
}

impl LxcMap {
    /// Open the global `lxc_map` from its pin. Errors if it has not been
    /// materialised yet (see
    /// [`Loader::load_global_maps`](crate::Loader::load_global_maps)).
    pub fn open(pins: &PinRoot) -> Res<Self> {
        Self::from_pin(&pins.global_map_dir(GlobalMap::LxcMap))
    }

    fn from_pin(path: &Path) -> Res<Self> {
        let data = MapData::from_pin(path).map_err(|e| LoaderError::MapOpen {
            path: path.to_path_buf(),
            src: e.to_string(),
        })?;
        let map = Map::from_map_data(data).map_err(|e| access(&e))?;
        let map = HashMap::try_from(map).map_err(|e| access(&e))?;
        Ok(Self { map })
    }

    /// Insert or overwrite the entry for `ip`.
    pub fn upsert_endpoint(&mut self, ip: Ipv4Addr, info: EndpointInfo) -> Res<()> {
        self.map
            .insert(Ipv4Key::from_addr(ip), info, 0)
            .map_err(|e| access(&e))
    }

    /// Remove the entry for `ip`. Missing keys are not an error.
    pub fn remove_endpoint(&mut self, ip: Ipv4Addr) -> Res<()> {
        match self.map.remove(&Ipv4Key::from_addr(ip)) {
            Ok(()) | Err(MapError::KeyNotFound) => Ok(()),
            Err(e) => Err(access(&e)),
        }
    }

    /// Look up the entry for `ip`, if any.
    pub fn get_endpoint(&self, ip: Ipv4Addr) -> Res<Option<EndpointInfo>> {
        match self.map.get(&Ipv4Key::from_addr(ip), 0) {
            Ok(info) => Ok(Some(info)),
            Err(MapError::KeyNotFound) => Ok(None),
            Err(e) => Err(access(&e)),
        }
    }

    /// Every `(ip, info)` currently in the table.
    pub fn endpoints(&self) -> Res<Vec<(Ipv4Addr, EndpointInfo)>> {
        self.map
            .iter()
            .map(|res| {
                res.map(|(key, info)| (key.to_addr(), info))
                    .map_err(|e| access(&e))
            })
            .collect()
    }
}

fn access(e: &MapError) -> LoaderError {
    LoaderError::MapAccess {
        map: LXC_MAP_NAME,
        src: e.to_string(),
    }
}
