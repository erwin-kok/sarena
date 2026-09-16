use aya_ebpf::{helpers::generated::bpf_ktime_get_ns, programs::TcContext};
use aya_log_ebpf::debug;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};
use sarena_ebpf_common::at;
use sarena_shared::{Ipv4Key, Ipv4KeyExt as _, OBS_POINT_CONTAINER_FORWARD};

use crate::{
    arp::process_arp,
    conntrack::{
        conntrack::{ct_create, ct_lookup},
        tuple::ConnTrackTuple,
    },
    endpoint::{get_endpoint_config, lookup_ipv4_endpoint},
    error::{Res, Verdict},
    ipv4::is_fragmented,
    local_delivery, metrics,
};

#[inline(always)]
pub fn try_from_container(ctx: TcContext) -> Res<Verdict> {
    let eth: &EthHdr = unsafe { at(&ctx, 0)? };
    let Ok(ether_type) = eth.ether_type() else {
        return Ok(Verdict::Pass);
    };

    let config = get_endpoint_config()?;
    debug!(
        &ctx,
        "endpoint config: {:mac}, ip: {:i}", config.mac, config.ipv4
    );

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
    let (fragmented, src_ip, dst_ip) = {
        let ip4: &Ipv4Hdr = unsafe { at(ctx, EthHdr::LEN)? };
        (
            is_fragmented(ip4),
            Ipv4Key::from_octets(ip4.src_addr),
            Ipv4Key::from_octets(ip4.dst_addr),
        )
    };

    if fragmented {
        debug!(ctx, "drop fragmented IP packet");
        return Ok(Verdict::Drop);
    }

    conntrack(ctx)?;

    {
        let eth: &EthHdr = unsafe { at(ctx, 0)? };
        debug!(
            ctx,
            "IPv4 -- dst-mac: {:mac}, src-mac: {:mac}, src-ip: {:i}, dst-ip: {:i}",
            eth.dst_addr,
            eth.src_addr,
            src_ip.to_addr(),
            dst_ip.to_addr(),
        );
    }

    if let Some(ep) = lookup_ipv4_endpoint(dst_ip) {
        metrics::update_metrics(ctx.len() as u64, OBS_POINT_CONTAINER_FORWARD);

        return local_delivery(ctx, ep);
    };

    Ok(Verdict::Pass)
}

#[inline(always)]
fn conntrack(ctx: &TcContext) -> Res<()> {
    // let now = unsafe { bpf_ktime_get_ns() };

    let tuple = ConnTrackTuple::new(ctx)?;
    tuple.print_key();

    match ct_lookup(&tuple, 0) {
        Some((v, e)) => {}
        None => {
            ct_create(&tuple, 0, None, 0)?;
        }
    }

    Ok(())
}
