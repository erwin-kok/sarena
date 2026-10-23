use aya_ebpf::programs::TcContext;
use aya_log_ebpf::debug;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};
use sarena_ebpf_common::ref_at;
use sarena_shared::{EndpointKey, OBS_POINT_CONTAINER_FORWARD};

use crate::{
    arp::process_arp,
    endpoint::{get_endpoint_config, lookup_endpoint},
    error::{Res, Verdict},
    ipv4::is_fragmented,
    local_delivery, metrics,
};

#[inline(always)]
pub fn try_from_container(ctx: TcContext) -> Res<Verdict> {
    let eth: &EthHdr = unsafe { ref_at(&ctx, 0)? };

    let Ok(ether_type) = eth.ether_type() else {
        return Ok(Verdict::Pass);
    };

    let config = get_endpoint_config()?;
    match ether_type {
        EtherType::Ipv4 => process_ipv4(&ctx),
        EtherType::Arp => process_arp(&ctx, config),
        _ => Ok(Verdict::Drop),
    }
}

#[inline(always)]
pub fn try_to_container(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

#[inline(always)]
fn process_ipv4(ctx: &TcContext) -> Res<Verdict> {
    let ip4: &Ipv4Hdr = unsafe { ref_at(ctx, EthHdr::LEN)? };

    let fragmented = is_fragmented(ip4);
    let src_ip = ip4.src_addr;
    let dst_ip = ip4.dst_addr;

    if fragmented {
        debug!(ctx, "drop fragmented IP packet");
        return Ok(Verdict::Drop);
    }

    {
        let eth: &EthHdr = unsafe { ref_at(ctx, 0)? };
        debug!(
            ctx,
            "IPv4 -- dst-mac: {:mac}, src-mac: {:mac}, src-ip: {:i}, dst-ip: {:i}",
            eth.dst_addr,
            eth.src_addr,
            src_ip,
            dst_ip,
        );
    }

    if let Some(ep) = lookup_endpoint(EndpointKey::from_ipv4_octets(dst_ip)) {
        metrics::update_metrics(ctx.len() as u64, OBS_POINT_CONTAINER_FORWARD);

        return local_delivery(ctx, ep);
    };

    Ok(Verdict::Pass)
}
