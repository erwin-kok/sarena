use std::{fs, path::Path};

use aya::programs::{
    LinkOrder, SchedClassifier, TcAttachType,
    links::{FdLink, PinnedLink},
    tc::{SchedClassifierLink, TcAttachOptions},
};
use libbpf_sys::{BPF_TCX_EGRESS, BPF_TCX_INGRESS};

use crate::{
    InfraError, Link, PinnedTcxProgram, Res,
    bpf::{LinkAttachPoint, link_attach_point},
};

pub fn upsert_tcx_program(
    device: &impl Link,
    prog: &mut SchedClassifier,
    bpffs_dir: impl AsRef<Path>,
    attach_type: TcAttachType,
) -> Res<PinnedTcxProgram> {
    if let Ok(program) = update_tcx(prog, &bpffs_dir) {
        return Ok(program);
    }
    attach_tcx(device, prog, &bpffs_dir, attach_type)
}

fn update_tcx(prog: &mut SchedClassifier, bpffs_dir: impl AsRef<Path>) -> Res<PinnedTcxProgram> {
    let name = prog_name(prog);
    let pin_path = bpffs_dir.as_ref().join(&name);
    let link_id = update_link(pin_path, prog)?;
    Ok(PinnedTcxProgram { name, link_id })
}

fn attach_tcx(
    device: &impl Link,
    prog: &mut SchedClassifier,
    bpffs_dir: impl AsRef<Path>,
    attach_type: TcAttachType,
) -> Res<PinnedTcxProgram> {
    let name = prog_name(prog);
    let options = TcAttachOptions::TcxOrder(LinkOrder::last());
    let link_id = prog.attach_with_options(device.ifname(), attach_type, options)?;
    let link = prog.take_link(link_id)?;
    let fd_link: FdLink = link.try_into()?;
    // Grab the kernel's own numeric ID for this link *before* `.pin()`
    // consumes it -- this is what `has_tcx_link` will later match against,
    // instead of the (truncated, non-unique) program name.
    let link_id = fd_link.info()?.id();
    let pin_path = bpffs_dir.as_ref().join(&name);
    fd_link.pin(&pin_path)?;
    Ok(PinnedTcxProgram { name, link_id })
}

pub fn detach_tcx(bpffs_dir: impl AsRef<Path>, program: &PinnedTcxProgram) -> Res<()> {
    let pin_path = bpffs_dir.as_ref().join(&program.name);
    unpin_link(pin_path)
}

pub fn has_tcx_link(
    device: &impl Link,
    program: &PinnedTcxProgram,
    attach_type: TcAttachType,
) -> Res<bool> {
    let attach_type = match attach_type {
        TcAttachType::Ingress => BPF_TCX_INGRESS,
        TcAttachType::Egress => BPF_TCX_EGRESS,
        TcAttachType::Custom(c) => c,
    };
    Ok(matches!(
        link_attach_point(program.link_id)?,
        Some(LinkAttachPoint::Tcx { ifindex, attach_type: actual })
            if ifindex == device.ifindex() && actual == attach_type
    ))
}

/// Aya truncates this to 16 bytes (`BPF_OBJ_NAME_LEN`); it's used to build
/// the pin path, not as a unique identifier -- see [`PinnedTcxProgram`].
fn prog_name(prog: &SchedClassifier) -> String {
    let info = prog.info().unwrap();
    info.name_as_str().unwrap_or("[unknown]").to_owned()
}

pub(crate) fn update_link(pin_path: std::path::PathBuf, prog: &mut SchedClassifier) -> Res<u32> {
    let pinned_link = PinnedLink::from_pin(&pin_path).inspect_err(|_| {
        let _ = fs::remove_file(&pin_path);
    })?;
    let fd_link: FdLink = pinned_link.into();
    let link_id = fd_link.info()?.id();
    let link: SchedClassifierLink = fd_link.try_into()?;
    let link_id_internal = prog.attach_to_link(link)?;
    // `attach_to_link` transfers ownership of the link into `prog`'s
    // internal map, keeping a second, independent fd reference to the
    // underlying bpf_link alive for as long as `prog` itself lives. We
    // don't want that -- the pin is meant to be the sole source of truth
    // for "is this attached" -- so reclaim and drop it immediately,
    // exactly like `attach_tcx` already does after a fresh attach.
    let _ = prog.take_link(link_id_internal)?;
    Ok(link_id)
}

pub(crate) fn unpin_link(pin_path: std::path::PathBuf) -> Res<()> {
    let pinned_link = PinnedLink::from_pin(&pin_path).inspect_err(|_| {
        let _ = fs::remove_file(&pin_path);
    })?;
    pinned_link.unpin().map_err(|e| InfraError::Io {
        context: format!("unpin link {}", pin_path.to_string_lossy()),
        source: e,
    })?;
    Ok(())
}
