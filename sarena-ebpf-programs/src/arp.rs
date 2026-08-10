use aya_ebpf::{bindings::tcx_action_base::TCX_NEXT, programs::TcContext};
use aya_log_ebpf::{debug, info};
use network_types::{arp::ArpHdr, eth::EthHdr};
use sarena_ebpf_common::{bpf_memcmp, ptr_at};
use sarena_shared::EndpointConfig;

use crate::{
    constants::{ARPHRD_ETHER, ARPOP_REPLY, ARPOP_REQUEST, ETH_BROADCAST},
    skb::{ctx_get_ifindex, ctx_redirect_peer},
};

#[inline(always)]
pub fn process_arp(ctx: &TcContext, config: &EndpointConfig) -> Result<i32, ()> {
    let ethhdr: *const EthHdr = unsafe { ptr_at(&ctx, 0)? };
    let arphdr: *const ArpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };

    if !arp_matches(&ctx, ethhdr, arphdr, &config.mac) {
        return Err(());
    }

    let eth = unsafe { &*ethhdr };
    let arp = unsafe { &*arphdr };
    let smac = eth.src_addr;
    let spa = arp.spa();
    let tpa = arp.tpa();

    info!(
        &ctx,
        "endpoint config: {:mac}, ip: {:i}", config.mac, config.ipv4
    );

    info!(&ctx, "smac: {:mac}, spa: {:i}, tpa: {:i}", smac, spa, tpa);

    if tpa == config.ipv4.octets() {
        return Ok(TCX_NEXT);
    }

    info!(
        &ctx,
        "arp: who-has {:i}? (default) replying with {:mac}", tpa, config.mac
    );

    let eth_mut = ethhdr as *mut EthHdr;
    let arp_mut = arphdr as *mut ArpHdr;
    arp_prepare_response(eth_mut, arp_mut, config.mac, tpa, smac, spa);

    let ifindex = ctx_get_ifindex(ctx);
    let ret = ctx_redirect_peer(ifindex, 0);
    Ok(ret as i32)
}

fn arp_matches(ctx: &TcContext, eth: *const EthHdr, arp: *const ArpHdr, mac: &[u8; 6]) -> bool {
    let eth = unsafe { &*eth };
    let arp = unsafe { &*arp };
    let dmac = eth.dst_addr;

    debug!(
        &ctx,
        "dst mac: {:mac}, src mac: {:mac}", eth.dst_addr, eth.src_addr
    );

    arp.oper() == ARPOP_REQUEST
        && arp.htype() == ARPHRD_ETHER
        && (eth_is_bcast(&dmac) || bpf_memcmp(dmac.as_ptr(), mac.as_ptr(), 6) == 0)
}

#[inline(always)]
fn eth_is_bcast(a: &[u8; 6]) -> bool {
    bpf_memcmp(a.as_ptr(), ETH_BROADCAST.as_ptr(), ETH_BROADCAST.len()) == 0
}

#[inline(always)]
fn arp_prepare_response(
    eth_mut: *mut EthHdr,
    arp_mut: *mut ArpHdr,
    smac: [u8; 6],
    spa: [u8; 4],
    dmac: [u8; 6],
    tpa: [u8; 4],
) {
    unsafe {
        (*eth_mut).src_addr = smac;
        (*eth_mut).dst_addr = dmac;

        (*arp_mut).set_oper(ARPOP_REPLY);
        (*arp_mut).set_sha(smac);
        (*arp_mut).set_spa(spa);
        (*arp_mut).set_tha(dmac);
        (*arp_mut).set_tpa(tpa);
    }
}
