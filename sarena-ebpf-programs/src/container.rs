use aya_ebpf::{
    bindings::tcx_action_base::TCX_PASS, macros::map, maps::Array, programs::TcContext,
};
use network_types::eth::{EthHdr, EtherType};
use sarena_ebpf_common::ptr_at;
use sarena_shared::EndpointConfig;

use crate::arp::process_arp;

#[map(name = "endpoint_config")]
static ENDPOINT_CONFIG: Array<EndpointConfig> = Array::pinned(1, 0);

#[inline(always)]
pub fn try_from_container(ctx: TcContext) -> Result<i32, ()> {
    let ethhdr: *const EthHdr = unsafe { ptr_at(&ctx, 0)? };

    let Ok(ether_type) = unsafe { *ethhdr }.ether_type() else {
        return Ok(TCX_PASS);
    };

    let config = ENDPOINT_CONFIG.get(0).ok_or(())?;

    let result = match ether_type {
        EtherType::Ipv4 => process_ipv4(&ctx)?,
        EtherType::Arp => process_arp(&ctx, config)?,
        _ => TCX_PASS,
    };
    Ok(result)
}

#[inline(always)]
pub fn try_to_container(_ctx: TcContext) -> Result<i32, ()> {
    Ok(TCX_PASS)
}

#[inline(always)]
fn process_ipv4(_ctx: &TcContext) -> Result<i32, ()> {
    Ok(TCX_PASS)
}
