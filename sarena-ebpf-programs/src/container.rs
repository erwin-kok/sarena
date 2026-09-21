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
        conntrack::{ConnTrackVerdict, ct_create, ct_lookup, ct_update},
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
    let ip4: &Ipv4Hdr = unsafe { at(ctx, EthHdr::LEN)? };

    let fragmented = is_fragmented(ip4);
    let src_ip = Ipv4Key::from_octets(ip4.src_addr);
    let dst_ip = Ipv4Key::from_octets(ip4.dst_addr);

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
        Some((ConnTrackVerdict::Seen(dir), _)) => {
            let _ = ct_update(&tuple, dir, 0);
        }
        _ if tuple.is_related => {} /* ICMP error about a flow we don't track: pass through */
        // untouched
        _ => {
            ct_create(&tuple, 0, None, 0)?;
        }
    }

    Ok(())
}
