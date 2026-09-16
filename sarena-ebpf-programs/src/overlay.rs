use aya_ebpf::programs::TcContext;

use crate::error::{Res, Verdict};

#[inline(always)]
pub fn try_from_overlay(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

#[inline(always)]
pub fn try_to_overlay(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}
