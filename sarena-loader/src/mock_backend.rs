use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use sarena_infra::{Link as _, mock_link::MockLink};

use crate::{
    backend::BpfBackend,
    endpoint::EndpointId,
    error::{LoaderError, Res},
    manifest::Hook,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    ResolveLink(EndpointId),
    LoadInstance(Vec<(String, PathBuf)>),
    EnsureAttached {
        program: &'static str,
        hook: Hook,
        ifname: String,
    },
    UnpinMap(PathBuf),
    RemovePinDir(PathBuf),
}

#[derive(Default)]
pub struct MockBackend {
    pub calls: Vec<Call>,
    pub maps: HashMap<String, PathBuf>,
    pub links: HashSet<PathBuf>,
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
                return Err(LoaderError::Injected(msg));
            }
        }
        Ok(())
    }
}

impl BpfBackend for MockBackend {
    type Instance = MockInstance;
    type LinkType = MockLink;

    fn resolve_link(&mut self, id: &EndpointId) -> Res<Self::LinkType> {
        self.tick()?;
        self.calls.push(Call::ResolveLink(id.clone()));
        Ok(MockLink {
            ifname: id.link_name(),
            ..Default::default()
        })
    }

    fn load_instance(&mut self, maps: &HashMap<String, PathBuf>) -> Res<Self::Instance> {
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
        let path = link_pin_dir.join(format!("{}-{}", link.ifname(), program_name));
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
}
