use aya_ebpf::{
    btf_maps::{LruHashMap, PerCpuArray},
    helpers::bpf_printk,
    macros::btf_map,
    programs::TcContext,
};
use network_types::{
    eth::EthHdr,
    icmp::Icmpv4Hdr,
    ip::{IpProto, Ipv4Hdr},
    sctp::SctpHdr,
    tcp::TcpHdr,
    udp::UdpHdr,
};
use num_traits::FromPrimitive as _;
use sarena_ebpf_common::ptr_at;
use sarena_shared::{Ipv4Key, Ipv4KeyExt as _};

use crate::error::{EbpfError::UnsupportedProtocol, Res};

const CONNTRACK_MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy)]
pub struct TcpFlags {
    pub syn: bool,
    pub ack: bool,
    pub fin: bool,
    pub rst: bool,
}

impl TcpFlags {
    fn new(syn: bool, ack: bool, fin: bool, rst: bool) -> Self {
        Self { syn, ack, fin, rst }
    }
}

#[derive(Clone, Copy)]
pub struct ConnTrackInfo {
    pub key: ConnTrackKey,
    pub proto: IpProto,
    pub flags: Option<TcpFlags>,
}

pub enum ConnTrackStatus {
    New,
    Established,
    Reply,
    Related,
}

pub enum ConnTrackAction {
    Create,
    Close,
    Unspecified,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConnTrackKey {
    dst_addr: Ipv4Key,
    src_addr: Ipv4Key,
    dst_port: u16,
    src_port: u16,
    protocol: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConnTrackEntry {
    pub packets: u64,
}

#[btf_map(name = "conntrack_tcp_buffer")]
static CONNTRACK_TCP_BUFFER: LruHashMap<ConnTrackKey, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

#[btf_map(name = "conntrack_any_buffer")]
static CONNTRACK_ANY_BUFFER: LruHashMap<ConnTrackKey, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0> =
    LruHashMap::new();

impl ConnTrackInfo {
    #[inline(always)]
    pub fn new(ctx: &TcContext) -> Res<Self> {
        let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
        let ipv4 = unsafe { &*ipv4hdr };

        let proto = ipv4.proto()?;
        let ihl = ipv4.ihl() as usize;

        let (src_port, dst_port, flags) = match proto {
            IpProto::Udp => {
                let udphdr: *const UdpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let udp = unsafe { &*udphdr };
                (udp.src_port(), udp.dst_port(), None)
            }

            IpProto::Tcp => {
                let tcphdr: *const TcpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let tcp = unsafe { &*tcphdr };
                (
                    u16::from_be_bytes(tcp.source),
                    u16::from_be_bytes(tcp.dest),
                    Some(TcpFlags::new(
                        tcp.syn() != 0,
                        tcp.ack() != 0,
                        tcp.fin() != 0,
                        tcp.rst() != 0,
                    )),
                )
            }

            IpProto::Sctp => {
                let sctphdr: *const SctpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let sctp = unsafe { &*sctphdr };
                (
                    u16::from_be_bytes(sctp.src),
                    u16::from_be_bytes(sctp.dst),
                    None,
                )
            }

            // ICMP has no port concept -- 0/0 is the conventional placeholder
            // (Linux conntrack does the same). Still bounds-check that the
            // header is actually present before accepting the packet.
            IpProto::Icmp => {
                let _icmphdr: *const Icmpv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                (0, 0, None)
            }

            _ => return Err(UnsupportedProtocol(proto as u8)),
        };

        Ok(Self {
            key: ConnTrackKey::new(ipv4, src_port, dst_port, proto),
            proto,
            flags,
        })
    }

    pub fn lookup(&self) {
        let action = self.select_tcp_action();
        let ct_map = if self.proto == IpProto::Tcp {
            &CONNTRACK_TCP_BUFFER
        } else {
            &CONNTRACK_ANY_BUFFER
        };
        self.__lookup(ct_map);
    }

    fn select_tcp_action(&self) -> ConnTrackAction {
        match self.flags {
            None => ConnTrackAction::Unspecified,
            Some(flags) => {
                if flags.rst || flags.fin {
                    ConnTrackAction::Close
                } else if flags.syn && !flags.ack {
                    ConnTrackAction::Create
                } else {
                    ConnTrackAction::Unspecified
                }
            }
        }
    }

    fn __lookup(
        &self,
        map: &LruHashMap<ConnTrackKey, ConnTrackEntry, CONNTRACK_MAX_ENTRIES, 0>,
    ) -> ConnTrackStatus {
        ConnTrackStatus::New
    }
}

impl ConnTrackKey {
    pub fn new(ipv4: &Ipv4Hdr, src_port: u16, dst_port: u16, proto: IpProto) -> Self {
        Self {
            dst_addr: Ipv4Key::from_octets(ipv4.dst_addr),
            src_addr: Ipv4Key::from_octets(ipv4.src_addr),
            dst_port,
            src_port,
            protocol: proto as u8,
        }
    }

    pub fn reverse(&self) -> Self {
        Self {
            dst_addr: self.src_addr,
            src_addr: self.dst_addr,
            dst_port: self.src_port,
            src_port: self.dst_port,
            protocol: self.protocol,
        }
    }

    #[inline(always)]
    pub fn print_key(&self) {
        let src = self.src_addr.octets();
        let dst = self.dst_addr.octets();

        unsafe {
            bpf_printk!(
                c"conntrack proto=%d %u.%u.%u.%u:%u -> %u.%u.%u.%u:%u",
                self.protocol,
                src[0],
                src[1],
                src[2],
                src[3],
                self.src_port,
                dst[0],
                dst[1],
                dst[2],
                dst[3],
                self.dst_port,
            );
        }
    }
}
