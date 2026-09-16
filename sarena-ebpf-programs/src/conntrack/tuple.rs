use aya_ebpf::{helpers::bpf_printk, programs::TcContext};
use bitflags::bitflags;
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
    conntrack::conntrack::{ConnTrackDirection, ConnTrackScope, TcpFlags},
    error::{EbpfError::UnsupportedProtocol, Res},
};

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
pub struct ConnTrackTuple {
    pub daddr: Ipv4Key,
    pub saddr: Ipv4Key,
    pub dport: u16,
    pub sport: u16,
    pub nexthdr: u8,
    pub flags: TupleFlags,
}

impl ConnTrackTuple {
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
                (udp.src_port(), udp.dst_port(), TcpFlags::NONE)
            }

            IpProto::Tcp => {
                let tcphdr: *const TcpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let tcp = unsafe { &*tcphdr };
                (
                    u16::from_be_bytes(tcp.source),
                    u16::from_be_bytes(tcp.dest),
                    TcpFlags::new(
                        tcp.syn() != 0,
                        tcp.ack() != 0,
                        tcp.fin() != 0,
                        tcp.rst() != 0,
                    ),
                )
            }

            IpProto::Sctp => {
                let sctphdr: *const SctpHdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let sctp = unsafe { &*sctphdr };
                (
                    u16::from_be_bytes(sctp.src),
                    u16::from_be_bytes(sctp.dst),
                    TcpFlags::NONE,
                )
            }

            // ICMP has no port concept -- 0/0 is the conventional placeholder
            // (Linux conntrack does the same). Still bounds-check that the
            // header is actually present before accepting the packet.
            IpProto::Icmp => {
                let icmphdr: *const Icmpv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN + ihl)? };
                let _icmp = unsafe { &*icmphdr };
                (0, 0, TcpFlags::NONE)
            }

            _ => return Err(UnsupportedProtocol(proto as u8)),
        };

        Ok(Self {
            daddr: Ipv4Key::from_octets(ipv4.dst_addr),
            saddr: Ipv4Key::from_octets(ipv4.src_addr),
            dport: dst_port,
            sport: src_port,
            nexthdr: proto as u8,
            flags: TupleFlags::OUT,
        })
    }

    #[inline]
    pub fn select_tuple_type(&mut self, direction: ConnTrackDirection, scope: ConnTrackScope) {
        if direction == ConnTrackDirection::Service {
            self.flags = TupleFlags::SERVICE;
            return;
        }
        let egress = direction == ConnTrackDirection::Egress;
        self.flags = match scope {
            ConnTrackScope::Forward => {
                if egress {
                    TupleFlags::OUT
                } else {
                    TupleFlags::IN
                }
            }
            ConnTrackScope::BiDir | ConnTrackScope::Reverse => {
                if egress {
                    TupleFlags::IN
                } else {
                    TupleFlags::OUT
                }
            }
        }
    }

    #[inline]
    pub fn reverse(&self) -> Self {
        Self {
            daddr: self.saddr,
            saddr: self.daddr,
            dport: self.sport,
            sport: self.dport,
            nexthdr: self.nexthdr,
            flags: self.flags,
        }
    }

    #[inline]
    pub fn print_key(&self) {
        let src = self.saddr.octets();
        let dst = self.daddr.octets();

        unsafe {
            bpf_printk!(
                c"conntrack proto=%d %u.%u.%u.%u:%u -> %u.%u.%u.%u:%u",
                self.nexthdr,
                src[0],
                src[1],
                src[2],
                src[3],
                self.sport,
                dst[0],
                dst[1],
                dst[2],
                dst[3],
                self.dport,
            );
        }
    }
}
