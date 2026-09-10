use std::path::Path;

use aya::maps::{Map, MapData, MapError, PerCpuHashMap};
use sarena_shared::{MetricsKey, MetricsValue};

use crate::{LoaderError, PinRoot, error::Res, maps::GlobalMap};

pub const METRICS_MAP_NAME: &str = "metrics_map";

pub struct MetricsMap {
    map: PerCpuHashMap<MapData, MetricsKey, MetricsValue>,
}

impl MetricsMap {
    pub fn open(pins: &PinRoot) -> Res<Self> {
        Self::from_pin(&pins.global_map_dir(GlobalMap::MetricsMap))
    }

    fn from_pin(path: &Path) -> Res<Self> {
        let data = MapData::from_pin(path).map_err(|e| LoaderError::MapOpen {
            path: path.to_path_buf(),
            src: e.to_string(),
        })?;
        let map = Map::from_map_data(data).map_err(|e| access(&e))?;
        let map = PerCpuHashMap::try_from(map).map_err(|e| access(&e))?;
        Ok(Self { map })
    }

    pub fn metrics(&self) -> impl Iterator<Item = Res<(MetricsKey, MetricsValue)>> + '_ {
        self.map.iter().map(|res| {
            let (key, per_cpu) = res.map_err(|e| access(&e))?;
            let value = per_cpu.iter().fold(
                MetricsValue {
                    packets: 0,
                    bytes: 0,
                },
                |acc, cpu| MetricsValue {
                    packets: acc.packets.saturating_add(cpu.packets),
                    bytes: acc.bytes.saturating_add(cpu.bytes),
                },
            );
            Ok((key, value))
        })
    }

    pub fn clear(&mut self) -> Res<()> {
        let keys: Vec<MetricsKey> = self
            .map
            .keys()
            .collect::<Result<_, _>>()
            .map_err(|e| access(&e))?;
        for key in keys {
            match self.map.remove(&key) {
                Ok(()) | Err(MapError::KeyNotFound) => {}
                Err(e) => return Err(access(&e)),
            }
        }
        Ok(())
    }
}

fn access(e: &MapError) -> LoaderError {
    LoaderError::MapAccess {
        map: METRICS_MAP_NAME,
        src: e.to_string(),
    }
}
