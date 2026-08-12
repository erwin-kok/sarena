use aya_ebpf::programs::TcContext;

use crate::error::{EbpfReturn, Res};

pub fn try_from_host(_ctx: TcContext) -> Res<EbpfReturn> {
    Ok(EbpfReturn::Pass)
}

pub fn try_to_host(_ctx: TcContext) -> Res<EbpfReturn> {
    Ok(EbpfReturn::Pass)
}
