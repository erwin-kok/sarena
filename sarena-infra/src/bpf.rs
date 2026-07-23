use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::{InfraError, Res};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkAttachPoint {
    /// A TCX (`BPF_LINK_TYPE_TCX`) link -- `attach_type` is
    /// `BPF_TCX_INGRESS`/`BPF_TCX_EGRESS`.
    Tcx { ifindex: u32, attach_type: u32 },
    /// A link-based XDP (`BPF_LINK_TYPE_XDP`) link.
    Xdp { ifindex: u32 },
}

pub fn link_attach_point(link_id: u32) -> Res<Option<LinkAttachPoint>> {
    // SAFETY: FFI call taking only a scalar argument; libbpf validates
    // `link_id` itself and returns a negative errno on failure rather than
    // reading or writing through any pointer we control.
    let fd = unsafe { libbpf_sys::bpf_link_get_fd_by_id(link_id) };
    if fd < 0 {
        let errno = -fd;
        if errno == nix::errno::Errno::ENOENT as i32 {
            return Ok(None);
        }
        return Err(InfraError::BpfQuery {
            context: "bpf_link_get_fd_by_id".to_owned(),
            errno,
        });
    }
    // SAFETY: `fd` was just returned by `bpf_link_get_fd_by_id` above, is
    // owned, and isn't used anywhere outside this function; wrapping it in
    // `OwnedFd` guarantees it's closed exactly once, including on early
    // return via `?` below.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };

    // SAFETY: `bpf_link_info` is a plain-old-data struct; zero-init plus
    // passing its real size as the length out-param is the same
    // `LIBBPF_OPTS`-style convention used elsewhere in this crate.
    let mut info: libbpf_sys::bpf_link_info = unsafe { std::mem::zeroed() };
    let mut info_len = std::mem::size_of::<libbpf_sys::bpf_link_info>() as u32;

    // SAFETY: `fd` is a valid, open bpf_link fd for the duration of this
    // call; `info`/`info_len` are correctly sized and libbpf only writes
    // fields it defines.
    let ret = unsafe {
        libbpf_sys::bpf_link_get_info_by_fd(fd.as_raw_fd(), &raw mut info, &raw mut info_len)
    };
    if ret != 0 {
        return Err(InfraError::BpfQuery {
            context: "bpf_link_get_info_by_fd".to_owned(),
            errno: -ret,
        });
    }

    // SAFETY: `info.type_` is what the kernel itself just populated, and is
    // exactly what tells us which union arm it also populated -- we only
    // ever read the arm matching it.
    Ok(match info.type_ {
        libbpf_sys::BPF_LINK_TYPE_TCX => Some(LinkAttachPoint::Tcx {
            ifindex: unsafe { info.__bindgen_anon_1.tcx.ifindex },
            attach_type: unsafe { info.__bindgen_anon_1.tcx.attach_type },
        }),
        libbpf_sys::BPF_LINK_TYPE_XDP => Some(LinkAttachPoint::Xdp {
            ifindex: unsafe { info.__bindgen_anon_1.xdp.ifindex },
        }),
        _ => None,
    })
}
