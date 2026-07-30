use std::{
    fs,
    path::{Path, PathBuf},
};

use aya::{
    programs::{
        LinkOrder, ProgramError, SchedClassifier, TcAttachType,
        links::{FdLink, LinkError, PinnedLink},
        tc::{SchedClassifierLink, TcAttachOptions},
    },
    sys::SyscallError,
};
use tracing::info;

use crate::{InfraError, Link, PinnedTcxProgram, Res};

pub fn upsert_tcx(
    device: &impl Link,
    prog: &mut SchedClassifier,
    bpffs_dir: impl AsRef<Path>,
    attach_type: TcAttachType,
) -> Res<PinnedTcxProgram> {
    let info = prog.info().unwrap();
    let prog_name = info.name_as_str().unwrap_or("[unknown]");
    match update_tcx(device, prog, prog_name, &bpffs_dir, prog_name) {
        Ok(program) => return Ok(program),
        // Nothing pinned yet (first-ever attach, or a defunct pin that
        // `update_tcx` already cleaned up below) - fall through to a
        // fresh attach.
        Err(InfraError::NotExist(_)) => {}
        // Any other error is unrecoverable - surface it rather than
        // silently retrying via a fresh attach, which would otherwise
        // mask real failures (and, for a still-live pin, fail anyway
        // with EEXIST when `attach_tcx` tries to pin over it).
        Err(e) => return Err(e),
    }
    attach_tcx(device, prog, prog_name, &bpffs_dir, attach_type)
}

pub fn detach_tcx_program(bpffs_dir: impl AsRef<Path>, prog_name: &str) -> Res<()> {
    let pin_path = bpffs_dir.as_ref().join(prog_name);
    unpin_link(&pin_path)?;
    info!(
        pin_path = %pin_path.to_string_lossy(),
        prog_name = %prog_name,
        "Program removed from device"
    );
    Ok(())
}

fn update_tcx(
    device: &impl Link,
    prog: &mut SchedClassifier,
    prog_name: &str,
    bpffs_dir: impl AsRef<Path>,
    name: &str,
) -> Res<PinnedTcxProgram> {
    let pin_path = bpffs_dir.as_ref().join(prog_name);
    let link_id = match update_link(&pin_path, prog) {
        Ok(link_id) => link_id,
        // The pinned link is defunct (its target device is gone) - the
        // program is no longer triggered, so the stale pin has to be
        // removed before a fresh attach can reuse this path. Reported
        // back as `NotExist` so the caller only has to look for one
        // "needs a fresh attach" case.
        Err(InfraError::NoLink(_)) => {
            fs::remove_file(&pin_path).map_err(|e| InfraError::Io {
                context: format!("unpinning defunct link {}", pin_path.to_string_lossy()),
                source: e,
            })?;
            info!(
                pin_path = %pin_path.to_string_lossy(),
                prog_name = %prog_name,
                "Unpinned defunct link for program"
            );
            return Err(InfraError::NotExist(pin_path));
        }
        Err(e) => return Err(e),
    };
    info!(
        pin_path = %pin_path.to_string_lossy(),
        prog_name = %prog_name,
        link = %device.ifname(),
        "Program updated on device using tcx"
    );
    Ok(PinnedTcxProgram {
        name: name.to_owned(),
        link_id,
    })
}

fn attach_tcx(
    device: &impl Link,
    prog: &mut SchedClassifier,
    prog_name: &str,
    bpffs_dir: impl AsRef<Path>,
    attach_type: TcAttachType,
) -> Res<PinnedTcxProgram> {
    fs::create_dir_all(&bpffs_dir).map_err(|e| InfraError::Io {
        context: format!(
            "creating bpffs link dir for tcx attachment to device {}",
            bpffs_dir.as_ref().display()
        ),
        source: e,
    })?;
    let options = TcAttachOptions::TcxOrder(LinkOrder::last());
    let link_id = prog.attach_with_options(device.ifname(), attach_type, options)?;
    let link = prog.take_link(link_id)?;
    let fd_link: FdLink = link.try_into()?;
    // Grab the kernel's own numeric ID for this link *before* `.pin()`
    // consumes it -- this is what `has_tcx_link` will later match against,
    // instead of the (truncated, non-unique) program name.
    let link_id = fd_link.info()?.id();
    let pin_path = bpffs_dir.as_ref().join(prog_name);
    fd_link.pin(&pin_path)?;
    info!(
        pin_path = %pin_path.to_string_lossy(),
        prog_name = %prog_name,
        link = %device.ifname(),
        "Program attached to device using tcx"
    );
    Ok(PinnedTcxProgram {
        name: prog_name.to_owned(),
        link_id,
    })
}

pub fn has_tcx(device: &impl Link, program: &str, attach_type: TcAttachType) -> Res<bool> {
    let (_revision, programs) = SchedClassifier::query_tcx(device.ifname(), attach_type)?;
    Ok(programs
        .into_iter()
        .any(|p| p.name_as_str().is_some_and(|name| name == program)))
}

fn update_link(pin_path: &PathBuf, prog: &mut SchedClassifier) -> Res<u32> {
    // `BPF_OBJ_GET` on a path with nothing pinned there fails with
    // `ENOENT` - the common case for a brand new device/program pair,
    // reported distinctly (`InfraError::NotExist`) so callers can tell
    // "nothing pinned yet, fall through to a fresh attach" apart from a
    // genuinely abnormal failure to open an existing pin.
    let pinned_link = PinnedLink::from_pin(pin_path).map_err(|e| match &e {
        LinkError::SyscallError(SyscallError { io_error, .. })
            if io_error.kind() == std::io::ErrorKind::NotFound =>
        {
            InfraError::NotExist(pin_path.clone())
        }
        _ => InfraError::from(e),
    })?;
    let fd_link: FdLink = pinned_link.into();
    let link_id = fd_link.info()?.id();
    let link: SchedClassifierLink = fd_link.try_into()?;
    // `bpf_link_update` (called by `attach_to_link`) returns `ENOLINK`
    // when the link's target device has been removed/unregistered
    // while the link stayed pinned - see `tcx_link_update` in the
    // kernel's `kernel/bpf/tcx.c`. Reported distinctly
    // (`InfraError::NoLink`) rather than folded into the generic
    // `ProgramError` wrapper, since it's a specific, actionable state
    // ("this pin is orphaned, clean it up") rather than an arbitrary
    // failure.
    let link_id_internal = prog.attach_to_link(link).map_err(|e| match &e {
        ProgramError::SyscallError(SyscallError { io_error, .. })
            if io_error.raw_os_error() == Some(nix::errno::Errno::ENOLINK as i32) =>
        {
            InfraError::NoLink(pin_path.clone())
        }
        _ => InfraError::from(e),
    })?;
    // `attach_to_link` transfers ownership of the link into `prog`'s
    // internal map, keeping a second, independent fd reference to the
    // underlying bpf_link alive for as long as `prog` itself lives. We
    // don't want that -- the pin is meant to be the sole source of truth
    // for "is this attached" -- so reclaim and drop it immediately,
    // exactly like `attach_tcx` already does after a fresh attach.
    let _ = prog.take_link(link_id_internal)?;
    Ok(link_id)
}

fn unpin_link(pin_path: &PathBuf) -> Res<()> {
    let pinned_link = PinnedLink::from_pin(pin_path).inspect_err(|_| {
        let _ = fs::remove_file(pin_path);
    })?;
    pinned_link.unpin().map_err(|e| InfraError::Io {
        context: format!("unpin link {}", pin_path.to_string_lossy()),
        source: e,
    })?;
    Ok(())
}
