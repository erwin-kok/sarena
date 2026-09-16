use aya_ebpf::{btf_maps::LruHashMap, macros::btf_map, programs::TcContext};
use bitflags::bitflags;
use network_types::{
    eth::EthHdr,
    ip::{IpProto, Ipv4Hdr},
};
use sarena_ebpf_common::ptr_at;
use sarena_shared::Ipv4Key;

use crate::{conntrack::tuple::ConnTrackTuple, error::Res};

const CONNTRACK_MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy, PartialEq)]
pub enum ConnTrackDirection {
    Egress,
    Ingress,
    Service,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnTrackStatus {
    New,
    Established,
    Reply,
    Related,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnTrackAction {
    Create,
    Close,
    Unspecified,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ConnTrackScope {
    Forward,
    Reverse,
    BiDir,
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct TupleFlags: u8 {
        const OUT     = 0; // Outgoing flow
        const IN      = 1; // Incoming flow
        const RELATED = 2; // Flow represents related packets
        const SERVICE = 4; // Flow represents packets to service
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConnTrackKey {
    pub addr_lo: Ipv4Key,
    pub addr_hi: Ipv4Key,
    pub port_lo: u16,
    pub port_hi: u16,
    pub proto: u8,
    pub _pad: [u8; 3],
}

impl ConnTrackKey {
    /// Canonical key for a flow between (a_addr,a_port) and (b_addr,b_port).
    /// Order-independent: swapping the two arguments produces the same key.
    pub fn new(a_addr: u32, a_port: u16, b_addr: u32, b_port: u16, proto: u8) -> Self {
        if (a_addr, a_port) <= (b_addr, b_port) {
            Self {
                addr_lo: a_addr,
                port_lo: a_port,
                addr_hi: b_addr,
                port_hi: b_port,
                proto,
                _pad: [0; 3],
            }
        } else {
            Self {
                addr_lo: b_addr,
                port_lo: b_port,
                addr_hi: a_addr,
                port_hi: a_port,
                proto,
                _pad: [0; 3],
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConnTrackEntry {
    pub orig_src_addr: Ipv4Key,
    pub orig_dst_addr: Ipv4Key,
    pub orig_src_port: u16,
    pub orig_dst_port: u16,

    pub expires_ns: u64, // active-expiry check, independent of LRU eviction

    pub packets: u64,
    pub rx_closing: bool,
    pub tx_closing: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlowDir {
    Original,
    Reply,
}

impl FlowDir {
    #[inline(always)]
    pub fn of(entry: &ConnTrackEntry, pkt_src_addr: Ipv4Key, pkt_src_port: u16) -> Self {
        if pkt_src_addr == entry.orig_src_addr && pkt_src_port == entry.orig_src_port {
            FlowDir::Original
        } else {
            FlowDir::Reply
        }
    }
}

pub enum ConnTrackVerdict {
    New,
    Seen(FlowDir),
    Related(FlowDir),
}

#[btf_map(name = "conntrack_map")]
static CONNTRACK_MAP: LruHashMap<ConnTrackKey, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

pub fn ct_lookup(t: &ConnTrackTuple, now: u64) -> Option<(ConnTrackVerdict, ConnTrackEntry)> {
    let key = ConnTrackKey::new(t.src_addr, t.src_port, t.dst_addr, t.dst_port, t.proto);
    let entry = unsafe { CONNTRACK_MAP.get(&key) }?;
    if now > entry.expires_ns {
        return None;
    }

    let dir = FlowDir::of(entry, t.src_addr, t.src_port);
    Some((ConnTrackVerdict::Seen(dir), *entry))
}
