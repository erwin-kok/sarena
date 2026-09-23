use aya_ebpf::{btf_maps::LruHashMap, macros::btf_map};
use bitflags::bitflags;
use sarena_shared::Ipv4Key;

use crate::{
    EbpfError,
    conntrack::{timeout::initial_timeout, tuple::ConnTrackTuple},
    error::Res,
};

const CONNTRACK_MAX_ENTRIES: usize = 4096;

#[btf_map(name = "conntrack_map")]
static CONNTRACK_MAP: LruHashMap<ConnTrackKey, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

#[repr(u8)]
#[derive(Clone, Copy, Default, PartialEq)]
pub enum ConnTrackState {
    #[default]
    New = 0,
    Established = 1,
    Closing = 2,
}

bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct SeenFlags: u8 {
        const SEEN_FIN_ORIG     = 0b001;
        const SEEN_FIN_REPLY    = 0b010;
        const SEEN_RST          = 0b100;
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
} // 16 bytes

impl ConnTrackKey {
    /// Canonical key for a flow between (a_addr,a_port) and (b_addr,b_port).
    /// Order-independent: swapping the two arguments produces the same key.
    #[inline(always)]
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
#[derive(Clone, Copy, Default)]
pub struct ConnTrackEntry {
    pub orig_src_addr: Ipv4Key,
    pub orig_dst_addr: Ipv4Key,
    pub orig_src_port: u16,
    pub orig_dst_port: u16,

    pub state: ConnTrackState,
    pub flags: SeenFlags,

    pub service_id: u32,
    pub nat_addr: u32,
    pub nat_port: u16,
    pub _pad: u16,

    pub created_ns: u64,
    pub last_seen_ns: u64,
    pub expires_ns: u64,
    pub packets_orig: u64,
    pub packets_reply: u64,
} // 64 bytes

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

#[inline(always)]
pub fn ct_lookup(tuple: &ConnTrackTuple, now: u64) -> Option<(ConnTrackVerdict, ConnTrackEntry)> {
    let key = ConnTrackKey::new(
        tuple.src_addr,
        tuple.src_port,
        tuple.dst_addr,
        tuple.dst_port,
        tuple.proto,
    );
    let entry = unsafe { CONNTRACK_MAP.get(&key) }?;
    if now > entry.expires_ns {
        return None;
    }

    let dir = FlowDir::of(entry, tuple.src_addr, tuple.src_port);
    Some((ConnTrackVerdict::Seen(dir), *entry))
}

#[inline(always)]
pub fn ct_create(
    tuple: &ConnTrackTuple,
    now: u64,
    nat: Option<(u32, u16)>,
    service_id: u32,
) -> Res<()> {
    let key = ConnTrackKey::new(
        tuple.src_addr,
        tuple.src_port,
        tuple.dst_addr,
        tuple.dst_port,
        tuple.proto,
    );
    let entry = ConnTrackEntry {
        orig_src_addr: tuple.src_addr,
        orig_dst_addr: tuple.dst_addr,
        orig_src_port: tuple.src_port,
        orig_dst_port: tuple.dst_port,
        state: ConnTrackState::New,
        flags: SeenFlags::empty(),
        service_id,
        nat_addr: nat.map(|n| n.0).unwrap_or(0),
        nat_port: nat.map(|n| n.1).unwrap_or(0),
        _pad: 0,
        created_ns: now,
        last_seen_ns: now,
        expires_ns: now + initial_timeout(tuple.proto),
        packets_orig: 1,
        packets_reply: 0,
    };
    CONNTRACK_MAP
        .insert(&key, &entry, 0)
        .map_err(EbpfError::MapError)
}

#[inline(always)]
pub fn ct_update(t: &ConnTrackTuple, dir: FlowDir, now: u64) -> Result<(), ()> {
    Ok(())
}
