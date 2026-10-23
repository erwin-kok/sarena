use std::{net::IpAddr, path::Path};

use aya::maps::{HashMap, Map, MapData, MapError};
use sarena_shared::{EndpointInfo, EndpointKey};

use crate::{
    EndpointPersist,
    error::{DataPlaneError, Res},
    loader::PinRoot,
    maps::GlobalMap,
};

/// Global endpoint lookup table: `EndpointKey -> EndpointInfo`.
pub const LXC_MAP_NAME: &str = "lxc_map";

#[derive(Debug)]
pub struct LxcMap {
    map: HashMap<MapData, EndpointKey, EndpointInfo>,
}

impl LxcMap {
    pub fn open(pins: &PinRoot) -> Res<Self> {
        Self::from_pin(&pins.global_map_dir(GlobalMap::LxcMap))
    }

    fn from_pin(path: &Path) -> Res<Self> {
        let data = MapData::from_pin(path).map_err(|e| DataPlaneError::MapOpen {
            path: path.to_path_buf(),
            src: e.to_string(),
        })?;
        let map = Map::from_map_data(data).map_err(|e| access(&e))?;
        let map = HashMap::try_from(map).map_err(|e| access(&e))?;
        Ok(Self { map })
    }

    /// Insert or overwrite the entry for `ep`.
    pub fn upsert_endpoint(&mut self, ep: &EndpointPersist, info: &EndpointInfo) -> Res<()> {
        if let Some(ipv4) = ep.ipv4 {
            self.map
                .insert(&EndpointKey::from_ipv4_octets(ipv4.octets()), info, 0)
                .map_err(|e| access(&e))?;
        }
        if let Some(ipv6) = ep.ipv6 {
            self.map
                .insert(&EndpointKey::from_ipv6_octets(ipv6.octets()), info, 0)
                .map_err(|e| access(&e))?;
        }
        Ok(())
    }

    /// Remove the entry for `ep`. Missing keys are not an error.
    pub fn remove_endpoint(&mut self, ep: &EndpointPersist) -> Res<()> {
        if let Some(ipv4) = ep.ipv4 {
            match self
                .map
                .remove(&EndpointKey::from_ipv4_octets(ipv4.octets()))
            {
                Ok(()) | Err(MapError::KeyNotFound) => Ok(()),
                Err(e) => Err(access(&e)),
            }?;
        }
        if let Some(ipv6) = ep.ipv6 {
            match self
                .map
                .remove(&EndpointKey::from_ipv6_octets(ipv6.octets()))
            {
                Ok(()) | Err(MapError::KeyNotFound) => Ok(()),
                Err(e) => Err(access(&e)),
            }?;
        }
        Ok(())
    }

    /// Remove the entry for `ip`. Missing keys are not an error.
    pub fn remove_endpoint_by_ip(&mut self, ip: IpAddr) -> Res<()> {
        match self.map.remove(&EndpointKey::from(ip)) {
            Ok(()) | Err(MapError::KeyNotFound) => Ok(()),
            Err(e) => Err(access(&e)),
        }
    }

    /// Look up the entry for `ip`, if any.
    pub fn get_endpoint(&self, ip: IpAddr) -> Res<Option<EndpointInfo>> {
        match self.map.get(&EndpointKey::from(ip), 0) {
            Ok(info) => Ok(Some(info)),
            Err(MapError::KeyNotFound) => Ok(None),
            Err(e) => Err(access(&e)),
        }
    }

    /// Every `(ip, info)` currently in the table.
    pub fn endpoints(&self) -> Res<Vec<(IpAddr, EndpointInfo)>> {
        self.map
            .iter()
            .map(|res| {
                let (key, info) = res.map_err(|e| access(&e))?;
                let ip = IpAddr::try_from(key).map_err(|e| DataPlaneError::MapAccess {
                    map: LXC_MAP_NAME,
                    src: e.to_string(),
                })?;
                Ok((ip, info))
            })
            .collect()
    }
}

fn access(e: &MapError) -> DataPlaneError {
    DataPlaneError::MapAccess {
        map: LXC_MAP_NAME,
        src: e.to_string(),
    }
}
