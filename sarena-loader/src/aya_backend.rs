use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use aya::{
    Ebpf, EbpfLoader,
    programs::tc::{SchedClassifier, TcAttachType},
};
use sarena_infra::{
    Link as _, NetlinkNetworkProvisioner, NetworkProvisioner, TcxAttach as _,
    netlink_link::NetlinkLink,
};

use crate::{
    backend::BpfBackend,
    error::{LoaderError, Res},
    manifest::Hook,
};

pub struct AyaBackend {
    object_path: PathBuf,
    globals_dir: PathBuf,
    provisioner: NetlinkNetworkProvisioner,
}

impl AyaBackend {
    pub fn new(object_path: impl Into<PathBuf>, globals_dir: impl Into<PathBuf>) -> Self {
        Self {
            object_path: object_path.into(),
            globals_dir: globals_dir.into(),
            provisioner: NetlinkNetworkProvisioner,
        }
    }

    fn sched_classifier<'a>(bpf: &'a mut Ebpf, name: &'static str) -> Res<&'a mut SchedClassifier> {
        let sched = bpf
            .program_mut(name)
            .ok_or(LoaderError::ProgramNotFound { name })?
            .try_into()
            .map_err(|_| LoaderError::ProgramLoad {
                name,
                src: "program section exists but is not a SchedClassifier (tc) program".into(),
            })?;
        Ok(sched)
    }
}

impl BpfBackend for AyaBackend {
    type Instance = Ebpf;
    type LinkType = NetlinkLink;

    fn resolve_link(&mut self, link: &str) -> Res<NetlinkLink> {
        // `NetworkProvisioner::get_link` is async (rtnetlink-based); this
        // method is called from the actor's own dedicated OS thread (see
        // `actor::LoaderHandle::spawn`), which is never a tokio worker, so
        // blocking here is fine. A one-shot current-thread runtime bridges
        // the two, the same way `sarena_infra::Netns::run` does internally.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| LoaderError::LinkResolve {
                link: link.to_string(),
                src: format!("failed to build link-resolution runtime: {e}"),
            })?;
        rt.block_on(self.provisioner.get_link(link))
            .map_err(|e| LoaderError::LinkResolve {
                link: link.to_string(),
                src: e.to_string(),
            })
    }

    fn load_instance(&mut self, maps: &HashMap<String, PathBuf>) -> Res<Ebpf> {
        let mut loader = EbpfLoader::new();
        let mut loader = loader.default_map_pin_directory(&self.globals_dir);
        for (name, path) in maps {
            loader = loader.map_pin_path(name.as_str(), path);
        }
        let bpf = loader
            .load_file(&self.object_path)
            .map_err(|e| LoaderError::ObjectLoad(e.to_string()))?;

        for name in maps.keys() {
            if bpf.map(name.as_str()).is_none() {
                return Err(LoaderError::MapNotFound { name: name.clone() });
            }
        }

        Ok(bpf)
    }

    fn ensure_attached(
        &mut self,
        instance: &mut Ebpf,
        program_name: &'static str,
        hook: Hook,
        link: &NetlinkLink,
        link_pin_dir: &Path,
    ) -> Res<()> {
        let attach_type = match hook {
            Hook::TcxIngress => TcAttachType::Ingress,
            Hook::TcxEgress => TcAttachType::Egress,
        };

        let prog = Self::sched_classifier(instance, program_name)?;
        prog.load().map_err(|e| LoaderError::ProgramLoad {
            name: program_name,
            src: e.to_string(),
        })?;

        let mut link = link.clone();
        link.upsert_tcx_program(prog, link_pin_dir, attach_type)
            .map_err(|e| LoaderError::Attach {
                name: program_name,
                ifname: link.ifname().to_string(),
                src: e.to_string(),
            })?;
        Ok(())
    }

    fn unpin_map(&mut self, path: &Path) -> Res<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent by contract
            Err(e) => Err(LoaderError::Unpin {
                path: path.to_path_buf(),
                src: e.to_string(),
            }),
        }
    }

    fn remove_pin_dir(&mut self, dir: &Path) -> Res<()> {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), // idempotent by contract
            Err(e) => Err(LoaderError::Unpin {
                path: dir.to_path_buf(),
                src: e.to_string(),
            }),
        }
    }

    fn list_pins(&self, prefix: &Path) -> Res<Vec<PathBuf>> {
        let mut out = Vec::new();
        walk_dir(prefix, prefix, &mut out).map_err(|e| LoaderError::ListPins {
            prefix: prefix.to_path_buf(),
            src: e.to_string(),
        })?;
        Ok(out)
    }
}

fn walk_dir(dir: &Path, prefix: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, prefix, out)?;
        } else if let Ok(rel) = path.strip_prefix(prefix) {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}
