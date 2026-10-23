use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    path::{Path, PathBuf},
};

use sarena_infra::{Link as _, mock_link::MockLink};
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{
    EndpointId, EndpointPersist,
    error::{DataPlaneError, Res},
    loader::{backend::BpfBackend, manifest::Hook},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    ResolveLink(String),
    LoadGlobalMaps(Vec<(String, PathBuf)>),
    LoadInstance(Vec<(String, PathBuf)>),
    EnsureAttached {
        program: &'static str,
        hook: Hook,
        ifname: String,
    },
    UnpinMap(PathBuf),
    RemovePinDir(PathBuf),
    SetEndpointConfig(String),
    UpsertEndpointInfo(EndpointId),
    RemoveEndpointInfo(IpAddr),
    StopLogging(String),
    StopAllLogging,
}

#[derive(Default)]
pub struct MockBackend {
    pub calls: Vec<Call>,
    pub maps: HashMap<String, PathBuf>,
    pub links: HashSet<PathBuf>,
    pub endpoint_infos: HashMap<IpAddr, EndpointInfo>,
    pub call_count: usize,
    pub fail_at: Option<(usize, String)>,
}

#[derive(Debug, Default)]
pub struct MockInstance;

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_call_index(&self) -> usize {
        self.call_count
    }

    fn tick(&mut self) -> Res<()> {
        let n = self.call_count;
        self.call_count += 1;
        if let Some((fail_n, _)) = &self.fail_at {
            if n == *fail_n {
                let (_, msg) = self.fail_at.take().unwrap();
                return Err(DataPlaneError::Injected(msg));
            }
        }
        Ok(())
    }
}

impl BpfBackend for MockBackend {
    type Instance = MockInstance;
    type LinkType = MockLink;

    fn resolve_link(&mut self, link: &str) -> Res<Self::LinkType> {
        self.tick()?;
        self.calls.push(Call::ResolveLink(link.to_string()));
        Ok(MockLink {
            ifname: link.to_string(),
            ..Default::default()
        })
    }

    fn load_global_maps(&mut self, maps: &HashMap<String, PathBuf>) -> Res<Self::Instance> {
        self.tick()?;
        self.calls.push(Call::LoadGlobalMaps(
            maps.iter()
                .map(|(name, path)| (name.clone(), path.clone()))
                .collect(),
        ));
        for (name, path) in maps {
            self.maps.insert(name.clone(), path.clone());
        }
        Ok(MockInstance)
    }

    fn load_instance(
        &mut self,
        _link: &str,
        maps: &HashMap<String, PathBuf>,
    ) -> Res<Self::Instance> {
        self.tick()?;
        self.calls.push(Call::LoadInstance(
            maps.iter()
                .map(|(name, path)| (name.clone(), path.clone()))
                .collect(),
        ));
        for (name, path) in maps {
            self.maps.insert(name.clone(), path.clone());
        }
        Ok(MockInstance)
    }

    fn ensure_attached(
        &mut self,
        _instance: &mut Self::Instance,
        program_name: &'static str,
        hook: Hook,
        link: &MockLink,
        link_pin_dir: &Path,
    ) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::EnsureAttached {
            program: program_name,
            hook,
            ifname: link.ifname().to_string(),
        });
        let path = link_pin_dir.join(program_name);
        self.links.insert(path);
        Ok(())
    }

    fn unpin_map(&mut self, path: &Path) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::UnpinMap(path.to_path_buf()));
        self.maps.retain(|_, p| p != path);
        Ok(())
    }

    fn remove_pin_dir(&mut self, dir: &Path) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::RemovePinDir(dir.to_path_buf()));
        self.maps.retain(|_, p| !p.starts_with(dir));
        self.links.retain(|p| !p.starts_with(dir));
        Ok(())
    }

    fn list_pins(&self, prefix: &Path) -> Res<Vec<PathBuf>> {
        Ok(self
            .maps
            .values()
            .chain(self.links.iter())
            .filter(|p| p.starts_with(prefix))
            .filter_map(|p| p.strip_prefix(prefix).ok().map(|p| p.to_path_buf()))
            .collect())
    }

    fn set_endpoint_config(&mut self, link: &str, _config: EndpointConfig) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::SetEndpointConfig(link.to_string()));
        Ok(())
    }

    fn upsert_endpoint_info(&mut self, endpoint: &EndpointPersist, info: &EndpointInfo) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::UpsertEndpointInfo(endpoint.id));
        for ip in endpoint.ips() {
            self.endpoint_infos.insert(ip, *info);
        }
        Ok(())
    }

    fn remove_endpoint_info(&mut self, ip: IpAddr) -> Res<()> {
        self.tick()?;
        self.calls.push(Call::RemoveEndpointInfo(ip));
        self.endpoint_infos.remove(&ip);
        Ok(())
    }

    fn list_endpoint_infos(&self) -> Res<HashMap<IpAddr, EndpointInfo>> {
        Ok(self.endpoint_infos.clone())
    }

    fn stop_logging(&mut self, link: &str) {
        self.calls.push(Call::StopLogging(link.to_string()));
    }

    fn stop_all_logging(&mut self) {
        self.calls.push(Call::StopAllLogging);
    }
}
