use core::mem;

use aya_ebpf::{cty::c_long, programs::TcContext};
use network_types::eth::EthHdr;

use crate::bpf_memcpy;

// ─── constants ───────────────────────────────────────────────────────────────

// 3 outer headers + {VXLAN, GENEVE} + 3 inner headers.
pub const PKT_BUILDER_LAYERS: usize = 7;
pub const MAX_PACKET_OFF: u64 = 0xffff;
pub const IPV6_DEFAULT_HOPLIMIT: u8 = 64;

// IPv6 next-header values for extension headers
pub const NEXTHDR_HOP: u8 = 0; // Hop-by-hop option header
pub const NEXTHDR_TCP: u8 = 6; // TCP segment
pub const NEXTHDR_UDP: u8 = 17; // UDP message
pub const NEXTHDR_IPV6: u8 = 41; // IPv6 in IPv6 
pub const NEXTHDR_ROUTING: u8 = 43; // Routing header
pub const NEXTHDR_FRAGMENT: u8 = 44; // Fragmentation/reassembly header
pub const NEXTHDR_GRE: u8 = 47; // GRE header
pub const NEXTHDR_ESP: u8 = 50; // Encapsulating security payload.
pub const NEXTHDR_AUTH: u8 = 51; // Authentication header
pub const NEXTHDR_ICMP: u8 = 58; // ICMP for IPv6
pub const NEXTHDR_NONE: u8 = 59; // No next header
pub const NEXTHDR_DEST: u8 = 60; // Destination options header
pub const NEXTHDR_SCTP: u8 = 132; // SCTP message
pub const NEXTHDR_MOBILITY: u8 = 135; // Mobility header

pub const NEXTHDR_MAX: u8 = 255;

pub const ETH_P_IP: u16 = 0x0800;
pub const ETH_P_IPV6: u16 = 0x86DD;
pub const ETH_P_ARP: u16 = 0x0806;

// ─── layer enum ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PktLayer {
    None,

    // L2 layers
    Eth,
    Dot1Q,

    // L3 layers
    Ipv4,
    Ipv6,
    Arp,

    // IPv6 extension headers
    Ipv6HopByHop,
    Ipv6Routing,
    Ipv6Auth,
    Ipv6Dest,
    Ipv6Fragment,

    // L4 layers
    Tcp,
    Udp,
    Icmp,
    Icmpv6,
    Sctp,
    Esp,
    Igmp,

    // Tunnel layers
    Geneve,
    Vxlan,

    // Raw payload
    Data,
}

pub struct PacketBuilder<'a> {
    pub ctx: &'a TcContext,
    pub cur_off: u64,
    pub layer_offsets: [u64; PKT_BUILDER_LAYERS],
    pub layers: [PktLayer; PKT_BUILDER_LAYERS],
}

// ─── core builder ────────────────────────────────────────────────────────────

impl<'a> PacketBuilder<'a> {
    /// Create a new packet builder attached to `ctx`.
    #[inline]
    #[must_use]
    pub fn new(ctx: &'a TcContext) -> Self {
        Self {
            ctx,
            cur_off: 0,
            layer_offsets: [0u64; PKT_BUILDER_LAYERS],
            layers: [PktLayer::None; PKT_BUILDER_LAYERS],
        }
    }

    /// Return the index of the first free (None) layer slot, or -1 if full.    
    #[inline]
    #[must_use]
    pub fn free_layer(&self) -> i32 {
        let mut i = 0usize;
        while i < PKT_BUILDER_LAYERS {
            if self.layers[i] == PktLayer::None {
                return i as i32;
            }
            i += 1;
        }
        -1
    }

    /// Reserve `len` bytes of uninitialised payload.
    #[inline]
    pub fn push_data_room(&mut self, len: i32) -> u64 {
        let needed = (self.cur_off + len as u64 - self.ctx.len() as u64) as i32;
        if ctx_adjust_room(self.ctx, needed).is_err() {
            return 0;
        }

        let data = self.ctx.data();
        let data_end = self.ctx.data_end();
        if data > data_end {
            return 0;
        }

        if self.cur_off as i64 >= MAX_PACKET_OFF as i64 - len as i64 {
            return 0;
        }

        let layer = data + self.cur_off as usize;
        let layer_idx = self.free_layer();
        if layer_idx < 0 {
            return 0;
        }

        self.layers[layer_idx as usize] = PktLayer::Data;
        self.layer_offsets[layer_idx as usize] = self.cur_off;
        self.cur_off += len as u64;
        layer as u64
    }

    /// Copy `data` into the packet as a payload.
    #[inline]
    pub fn push_data(&mut self, data: &[u8]) -> u64 {
        let pkt_data = self.push_data_room(data.len() as i32);
        if pkt_data == 0 {
            return 0;
        }

        let end = pkt_data + data.len() as u64;
        if end > self.ctx.data_end() as u64 {
            return 0;
        }

        bpf_memcpy(pkt_data as *mut u8, data.as_ptr(), data.len());

        pkt_data
    }

    #[inline]
    pub fn build(&self) {
        let mut i = 0usize;
        while i < PKT_BUILDER_LAYERS {
            match self.layers[i] {
                PktLayer::None => return, // end of stack
                PktLayer::Eth => self.finish_eth(i),
                _ => {}
            }
            i += 1;
        }
    }

    // ─── finish helpers ───────────────────────────────────────────────────────

    #[inline]
    fn finish_eth(&self, i: usize) {
        let layer_off = self.layer_offsets[i];
        if layer_off >= MAX_PACKET_OFF - mem::size_of::<EthHdr>() as u64 {
            return;
        }
        let data = self.ctx.data();
        let data_end = self.ctx.data_end();
        let eth_layer = data + layer_off as usize;
        if eth_layer + mem::size_of::<EthHdr>() > data_end {
            return;
        }
        if i + 1 >= PKT_BUILDER_LAYERS {
            return;
        }
        let eth_layer = eth_layer as *mut EthHdr;
        let eth_type = match self.layers[i + 1] {
            PktLayer::Ipv4 => ETH_P_IP.to_be(),
            PktLayer::Ipv6 => ETH_P_IPV6.to_be(),
            PktLayer::Arp => ETH_P_ARP.to_be(),
            _ => return,
        };
        unsafe { (*eth_layer).ether_type = eth_type };
    }
}

pub fn ctx_adjust_room(ctx: &TcContext, len_diff: i32) -> Result<(), c_long> {
    ctx.change_tail((ctx.len() as i32 + len_diff) as u32, 0)
}
