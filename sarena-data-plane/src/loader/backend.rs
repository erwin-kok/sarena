use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
};

use sarena_infra::Link;
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{EndpointPersist, error::Res, loader::manifest::Hook};

pub trait BpfBackend {
    type Instance;
    type LinkType: Link;

    fn resolve_link(&mut self, link: &str) -> Res<Self::LinkType>;
    fn load_global_maps(&mut self, maps: &HashMap<String, PathBuf>) -> Res<Self::Instance>;
    fn load_instance(&mut self, link: &str, maps: &HashMap<String, PathBuf>)
    -> Res<Self::Instance>;
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
    fn set_endpoint_config(&mut self, link: &str, config: EndpointConfig) -> Res<()>;
    fn upsert_endpoint_info(&mut self, endpoint: &EndpointPersist, info: &EndpointInfo) -> Res<()>;
    fn remove_endpoint_info(&mut self, ip: IpAddr) -> Res<()>;
    fn list_endpoint_infos(&self) -> Res<HashMap<IpAddr, EndpointInfo>>;
    fn stop_logging(&mut self, link: &str);
    fn stop_all_logging(&mut self);
}
