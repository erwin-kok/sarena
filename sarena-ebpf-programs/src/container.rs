use aya_ebpf::{
    bindings::tcx_action_base::TCX_PASS, macros::map, maps::Array, programs::TcContext,
};
use sarena_common::EndpointConfig;

#[map(name = "endpoint_config")]
static ENDPOINT_CONFIG: Array<EndpointConfig> = Array::pinned(1, 0);

pub fn try_from_container(_ctx: TcContext) -> Result<i32, ()> {
    Ok(TCX_PASS)
}

pub fn try_to_container(_ctx: TcContext) -> Result<i32, ()> {
    Ok(TCX_PASS)
}
