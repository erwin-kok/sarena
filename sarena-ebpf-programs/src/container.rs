use aya_ebpf::{macros::map, maps::Array, programs::TcContext};
use aya_log_ebpf::{debug, info};
use network_types::{
    eth::{EthHdr, EtherType},
    ip::Ipv4Hdr,
};
use sarena_ebpf_common::ptr_at;
use sarena_shared::{EndpointConfig, Ipv4Key, Ipv4KeyExt as _};

use crate::{
    arp::process_arp,
    conntrack::ConnTrackInfo,
    error::{EbpfError::InternalError, EbpfReturn, Res},
};

#[map(name = "endpoint_config")]
static ENDPOINT_CONFIG: Array<EndpointConfig> = Array::pinned(1, 0);

#[inline(always)]
pub fn try_from_container(ctx: TcContext) -> Res<EbpfReturn> {
    let ethhdr: *const EthHdr = unsafe { ptr_at(&ctx, 0)? };

    let Ok(ether_type) = unsafe { *ethhdr }.ether_type() else {
        return Ok(EbpfReturn::Pass);
    };

    let config = ENDPOINT_CONFIG
        .get(0)
        .ok_or(InternalError("endpoint does not have EndpointConfig"))?;

    debug!(
        &ctx,
        "endpoint config: {:mac}, ip: {:i}", config.mac, config.ipv4
    );

    let result = match ether_type {
        EtherType::Ipv4 => process_ipv4(&ctx, config)?,
        EtherType::Arp => process_arp(&ctx, config)?,
        _ => EbpfReturn::Pass,
    };
    Ok(result)
}

#[inline(always)]
pub fn try_to_container(_ctx: TcContext) -> Res<EbpfReturn> {
    Ok(EbpfReturn::Pass)
}

#[inline(always)]
fn process_ipv4(ctx: &TcContext, _config: &EndpointConfig) -> Res<EbpfReturn> {
    let ethhdr: *const EthHdr = unsafe { ptr_at(&ctx, 0)? };
    let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };

    let eth = unsafe { &*ethhdr };
    let ipv4 = unsafe { &*ipv4hdr };

    // If we have a fragmented IP --> DROP
    if is_fragmented(ipv4) {
        debug!(&ctx, "drop fragmented IP packet");
        return Ok(EbpfReturn::Drop);
    }

    let src_ip = Ipv4Key::from_octets(ipv4.src_addr);
    let dst_ip = Ipv4Key::from_octets(ipv4.dst_addr);

    let cn_info = ConnTrackInfo::new(ctx)?;
    cn_info.key.print_key();

    info!(
        &ctx,
        "IPv4 -- dst-mac: {:mac}, src-mac: {:mac}, src-ip: {:i}, dst-ip: {:i}",
        eth.dst_addr,
        eth.src_addr,
        src_ip.to_addr(),
        dst_ip.to_addr(),
    );

    Ok(EbpfReturn::Pass)
}

// If MF is set, or the frag_offset is nonzero, we have a IP fragment
fn is_fragmented(ipv4: &Ipv4Hdr) -> bool {
    (ipv4.frag_flags() & 0x1) != 0 || (ipv4.frag_offset() != 0)
}
