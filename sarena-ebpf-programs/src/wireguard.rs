use aya_ebpf::programs::TcContext;

use crate::error::{Res, Verdict};

#[inline(always)]
pub fn try_from_wireguard(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

#[inline(always)]
pub fn try_to_wireguard(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}
