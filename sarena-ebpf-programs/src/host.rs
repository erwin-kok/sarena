use aya_ebpf::{bindings::tcx_action_base::TCX_PASS, programs::TcContext};

pub fn try_from_host(_ctx: TcContext) -> Result<i32, i32> {
    Ok(TCX_PASS)
}

pub fn try_to_host(_ctx: TcContext) -> Result<i32, i32> {
    Ok(TCX_PASS)
}
