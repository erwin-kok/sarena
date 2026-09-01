use aya_ebpf::programs::TcContext;
use network_types::{eth::EthHdr, ip::Ipv4Hdr};
use sarena_ebpf_common::at_mut;
use sarena_shared::EndpointInfo;

use crate::{
    error::{EbpfError, Res, Verdict},
    skb::ctx_redirect,
};

/// Offset of the IPv4 header checksum field within the frame.
const IPV4_CSUM_OFF: usize = EthHdr::LEN + 10;

#[inline]
pub fn local_delivery(ctx: &TcContext, ep: *const EndpointInfo) -> Res<Verdict> {
    let src_mac = unsafe { (*ep).host_mac };
    let dst_mac = unsafe { (*ep).container_mac };

    ipv4_dec_ttl(ctx)?;
    rewrite_eth(ctx, src_mac, dst_mac)?;

    let ifindex = unsafe { (*ep).if_index };
    Ok(Verdict::Redirect(ctx_redirect(ifindex, 0) as i32))
}

#[inline]
fn ipv4_dec_ttl(ctx: &TcContext) -> Res<()> {
    let ip4: &mut Ipv4Hdr = unsafe { at_mut(ctx, EthHdr::LEN)? };

    if ip4.ttl <= 1 {
        return Err(EbpfError::TtlExceeded);
    }
    let old = ip4.ttl;
    let new = old - 1;
    ip4.ttl = new;

    ctx.l3_csum_replace(IPV4_CSUM_OFF, old as u64, new as u64, 2)
        .map_err(|_| EbpfError::CsumL3)?;
    Ok(())
}

#[inline]
fn rewrite_eth(ctx: &TcContext, src_mac: [u8; 6], dst_mac: [u8; 6]) -> Res<()> {
    let eth: &mut EthHdr = unsafe { at_mut(ctx, 0)? };
    eth.dst_addr = dst_mac;
    eth.src_addr = src_mac;
    Ok(())
}
