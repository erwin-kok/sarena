use aya_ebpf::programs::TcContext;
use aya_log_ebpf::debug;
use network_types::{arp::ArpHdr, eth::EthHdr};
use sarena_ebpf_common::{at, at_mut, bpf_memcmp};
use sarena_shared::EndpointConfig;

use crate::{
    constants::{ARPHRD_ETHER, ARPOP_REPLY, ARPOP_REQUEST, ETH_BROADCAST},
    error::{Res, Verdict},
    skb::{ctx_get_ifindex, ctx_redirect_peer},
};

#[inline]
pub fn process_arp(ctx: &TcContext, config: &EndpointConfig) -> Res<Verdict> {
    let (is_request, dst_mac, src_mac, sender_ip, target_ip) = {
        let eth: &EthHdr = unsafe { at(ctx, 0)? };
        let arp: &ArpHdr = unsafe { at(ctx, EthHdr::LEN)? };
        (
            arp.oper() == ARPOP_REQUEST && arp.htype() == ARPHRD_ETHER,
            eth.dst_addr,
            eth.src_addr,
            arp.spa(),
            arp.tpa(),
        )
    };

    debug!(ctx, "arp: dst mac {:mac}, src mac {:mac}", dst_mac, src_mac);

    let for_us =
        eth_is_bcast(&dst_mac) || bpf_memcmp(dst_mac.as_ptr(), config.mac.as_ptr(), 6) == 0;
    if !is_request || !for_us {
        return Ok(Verdict::Pass);
    }

    // Our own endpoint IP is answered by the stack, not here.
    if target_ip == config.ipv4.octets() {
        return Ok(Verdict::Next);
    }

    debug!(
        ctx,
        "arp: who-has {:i}? replying with {:mac}", target_ip, config.mac
    );

    write_arp_reply(ctx, config.mac, src_mac, target_ip, sender_ip)?;

    let ifindex = ctx_get_ifindex(ctx);
    Ok(Verdict::Redirect(ctx_redirect_peer(ifindex, 0) as i32))
}

#[inline]
fn eth_is_bcast(a: &[u8; 6]) -> bool {
    bpf_memcmp(a.as_ptr(), ETH_BROADCAST.as_ptr(), ETH_BROADCAST.len()) == 0
}

/// Turn the ARP request currently in the packet into a reply from us.
#[inline]
fn write_arp_reply(
    ctx: &TcContext,
    our_mac: [u8; 6],
    requester_mac: [u8; 6],
    our_ip: [u8; 4],
    requester_ip: [u8; 4],
) -> Res<()> {
    let eth: &mut EthHdr = unsafe { at_mut(ctx, 0)? };
    eth.src_addr = our_mac;
    eth.dst_addr = requester_mac;

    let arp: &mut ArpHdr = unsafe { at_mut(ctx, EthHdr::LEN)? };
    arp.set_oper(ARPOP_REPLY);
    arp.set_sha(our_mac);
    arp.set_spa(our_ip);
    arp.set_tha(requester_mac);
    arp.set_tpa(requester_ip);

    Ok(())
}
