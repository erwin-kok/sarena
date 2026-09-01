use aya_ebpf::programs::TcContext;

use crate::error::{Res, Verdict};

pub fn try_from_netdev(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

pub fn try_to_netdev(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}
