use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::{endpoint::EndpointId, error::Res, manifest::Hook};

pub trait BpfBackend {
    type Instance;
    type LinkType;

    fn resolve_link(&mut self, id: &EndpointId) -> Res<Self::LinkType>;
    fn load_instance(&mut self, maps: &HashMap<String, PathBuf>) -> Res<Self::Instance>;
    fn ensure_attached(
        &mut self,
        instance: &mut Self::Instance,
        program_name: &'static str,
        hook: Hook,
        link: &Self::LinkType,
        link_pin_dir: &Path,
    ) -> Res<()>;
    fn unpin_map(&mut self, path: &Path) -> Res<()>;
    fn remove_pin_dir(&mut self, dir: &Path) -> Res<()>;
    fn list_pins(&self, prefix: &Path) -> Res<Vec<PathBuf>>;
}
