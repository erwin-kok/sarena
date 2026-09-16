use aya_ebpf::{btf_maps::LruHashMap, macros::btf_map, programs::TcContext};
use bitflags::bitflags;
use network_types::{
    eth::EthHdr,
    ip::{IpProto, Ipv4Hdr},
};
use sarena_ebpf_common::ptr_at;

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
pub struct ConnTrackEntry {
    pub packets: u64,
    pub rx_closing: bool,
    pub tx_closing: bool,
}

#[btf_map(name = "conntrack_tcp_buffer")]
static CONNTRACK_TCP_BUFFER: LruHashMap<ConnTrackTuple, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

#[btf_map(name = "conntrack_any_buffer")]
static CONNTRACK_ANY_BUFFER: LruHashMap<ConnTrackTuple, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

#[derive(Clone, Copy)]
pub struct ConnTrackInfo {
    pub proto: IpProto,
}

impl ConnTrackInfo {
    #[inline(always)]
    pub fn new(ctx: &TcContext) -> Res<Self> {
        let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
        let ipv4 = unsafe { &*ipv4hdr };
        let proto = ipv4.proto()?;

        Ok(Self { proto })
    }
}

#[inline]
pub fn ct_lookup4(
    ctx: &TcContext,
    tuple: &mut ConnTrackTuple,
    direction: ConnTrackDirection,
    scope: ConnTrackScope,
) {
}
