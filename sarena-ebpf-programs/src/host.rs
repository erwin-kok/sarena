use aya_ebpf::programs::TcContext;
use aya_log_ebpf::debug;
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};
use sarena_ebpf_common::at;
use sarena_shared::{Ipv4Key, Ipv4KeyExt as _};

use crate::{
    endpoint::{get_endpoint_config, lookup_ipv4_endpoint},
    error::{Res, Verdict},
    ipv4::is_fragmented,
    local_delivery,
};

#[inline]
pub fn try_from_host(ctx: TcContext) -> Res<Verdict> {
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
        _ => Ok(Verdict::Pass),
    }
}

#[inline]
pub fn try_to_host(_ctx: TcContext) -> Res<Verdict> {
    Ok(Verdict::Pass)
}

#[inline]
fn process_ipv4(ctx: &TcContext) -> Res<Verdict> {
    let (fragmented, dst_ip) = {
        let ip4: &Ipv4Hdr = unsafe { at(ctx, EthHdr::LEN)? };
        (is_fragmented(ip4), Ipv4Key::from_octets(ip4.dst_addr))
    };

    if fragmented {
        debug!(ctx, "drop fragmented IP packet");
        return Ok(Verdict::Drop);
    }

    if let Some(ep) = lookup_ipv4_endpoint(dst_ip) {
        return local_delivery(ctx, ep);
    };

    Ok(Verdict::Pass)
}
