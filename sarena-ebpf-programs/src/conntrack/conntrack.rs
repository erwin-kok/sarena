use aya_ebpf::{btf_maps::LruHashMap, macros::btf_map, programs::TcContext};
use aya_log_ebpf::info;
use network_types::{
    eth::EthHdr,
    icmp::Icmpv4Hdr,
    ip::{IpProto, Ipv4Hdr},
    sctp::SctpHdr,
    tcp::TcpHdr,
    udp::UdpHdr,
};
use sarena_ebpf_common::ptr_at;
use sarena_shared::{Ipv4Key, Ipv4KeyExt as _};

use crate::{
    conntrack::tuple::ConnTrackTuple,
    error::{EbpfError::UnsupportedProtocol, Res},
};

const CONNTRACK_MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
}

impl TcpFlags {
    pub const NONE: Self = Self {
        syn: false,
        ack: false,
        fin: false,
        rst: false,
    };

    #[inline(always)]
    pub fn new(syn: bool, ack: bool, fin: bool, rst: bool) -> Self {
        Self { syn, ack, fin, rst }
    }
}

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
pub fn ct_lookup(
    ctx: &TcContext,
    tuple: &mut ConnTrackTuple,
    direction: ConnTrackDirection,
    scope: ConnTrackScope,
) {
}
