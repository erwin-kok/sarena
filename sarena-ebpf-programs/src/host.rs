use aya_ebpf::programs::TcContext;
use aya_log_ebpf::debug;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};
use sarena_ebpf_common::ref_at;
use sarena_shared::{EndpointKey, OBS_POINT_HOST_FORWARD};

use crate::{
    endpoint::lookup_endpoint,
    error::{Res, Verdict},
    ipv4::is_fragmented,
    local_delivery, metrics,
};

#[inline(always)]
pub fn try_from_host(ctx: TcContext) -> Res<Verdict> {
    let eth: &EthHdr = unsafe { ref_at(&ctx, 0)? };
    let Ok(ether_type) = eth.ether_type() else {
        return Ok(Verdict::Pass);
    };

    match ether_type {
        EtherType::Ipv4 => process_ipv4(&ctx),
        _ => Ok(Verdict::Pass),
    }
}

#[inline(always)]
pub fn try_to_host(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

#[inline(always)]
fn process_ipv4(ctx: &TcContext) -> Res<Verdict> {
    let ip4: &Ipv4Hdr = unsafe { ref_at(ctx, EthHdr::LEN)? };
    let fragmented = is_fragmented(ip4);

    if fragmented {
        debug!(ctx, "drop fragmented IP packet");
        return Ok(Verdict::Drop);
    }

    let dst_ip: EndpointKey = EndpointKey::from_ipv4_octets(ip4.dst_addr);
    if let Some(ep) = lookup_endpoint(dst_ip) {
        metrics::update_metrics(ctx.len() as u64, OBS_POINT_HOST_FORWARD);

        return local_delivery(ctx, ep);
    };

    Ok(Verdict::Pass)
}
