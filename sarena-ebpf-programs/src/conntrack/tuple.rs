use aya_ebpf::{bpf_printk, programs::TcContext};
use network_types::{
    eth::EthHdr,
    icmp::{Icmpv4Hdr, Icmpv4HdrData},
    ip::{IpProto, Ipv4Hdr},
    sctp::SctpHdr,
    tcp::TcpHdr,
    udp::UdpHdr,
};
use sarena_ebpf_common::ptr_at;
use sarena_shared::{Ipv4Key, Ipv4KeyExt as _};

use crate::{
    conntrack::tcp_flags::TcpFlags,
    error::{EbpfError::UnsupportedProtocol, Res},
};

#[derive(Clone, Copy)]
pub struct ConnTrackTuple {
    pub src_addr: Ipv4Key,
    pub dst_addr: Ipv4Key,
    pub src_port: u16,
    pub dst_port: u16,
    pub proto: u8,
    pub tcp_flags: Option<TcpFlags>,
}

impl ConnTrackTuple {
    #[inline(always)]
    pub fn new(ctx: &TcContext) -> Res<Self> {
        let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
        let ipv4 = unsafe { &*ipv4hdr };

        let proto = ipv4.proto()?;
        let ihl: usize = ipv4.ihl() as usize;

        let (src_port, dst_port, flags) = match proto {
            IpProto::Udp => Self::extract_for_udp(ctx, EthHdr::LEN + ihl)?,
            IpProto::Tcp => Self::extract_for_tcp(ctx, EthHdr::LEN + ihl)?,
            IpProto::Sctp => Self::extract_for_sctp(ctx, EthHdr::LEN + ihl)?,
            IpProto::Icmp => Self::extract_for_icmp(ctx, EthHdr::LEN + ihl)?,
            _ => return Err(UnsupportedProtocol(proto as u8)),
        };

        Ok(Self {
            dst_addr: Ipv4Key::from_octets(ipv4.dst_addr),
            src_addr: Ipv4Key::from_octets(ipv4.src_addr),
            dst_port,
            src_port,
            proto: proto as u8,
            tcp_flags: flags,
        })
    }

    #[inline(always)]
    pub fn print_key(&self) {
        let src = self.src_addr.octets();
        let dst = self.dst_addr.octets();
        unsafe {
            bpf_printk!(
                c"conntrack proto=%d %u.%u.%u.%u:%u -> %u.%u.%u.%u:%u",
                self.proto,
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

    #[inline(always)]
    fn extract_for_tcp(ctx: &TcContext, offset: usize) -> Res<(u16, u16, Option<TcpFlags>)> {
        let tcphdr: *const TcpHdr = unsafe { ptr_at(&ctx, offset)? };
        let tcp = unsafe { &*tcphdr };
        Ok((
            u16::from_be_bytes(tcp.source),
            u16::from_be_bytes(tcp.dest),
            Some(TcpFlags::from(&tcp)),
        ))
    }

    #[inline(always)]
    fn extract_for_udp(ctx: &TcContext, offset: usize) -> Res<(u16, u16, Option<TcpFlags>)> {
        let udphdr: *const UdpHdr = unsafe { ptr_at(&ctx, offset)? };
        let udp = unsafe { &*udphdr };
        Ok((udp.src_port(), udp.dst_port(), None))
    }

    #[inline(always)]
    fn extract_for_sctp(ctx: &TcContext, offset: usize) -> Res<(u16, u16, Option<TcpFlags>)> {
        let sctphdr: *const SctpHdr = unsafe { ptr_at(&ctx, offset)? };
        let sctp = unsafe { &*sctphdr };
        Ok((
            u16::from_be_bytes(sctp.src),
            u16::from_be_bytes(sctp.dst),
            None,
        ))
    }

    #[inline(always)]
    fn extract_for_icmp(ctx: &TcContext, offset: usize) -> Res<(u16, u16, Option<TcpFlags>)> {
        let icmphdr: *const Icmpv4Hdr = unsafe { ptr_at(&ctx, offset)? };
        let icmp = unsafe { &*icmphdr };
        let data = icmp.data()?;
        match data {
            Icmpv4HdrData::EchoReply(id_seq) | Icmpv4HdrData::Echo(id_seq) => {
                Ok((id_seq.id(), id_seq.id(), None))
            }

            Icmpv4HdrData::DestinationUnreachable(_) | Icmpv4HdrData::ParameterProblem(_) => {
                let inner_offset = offset + Icmpv4Hdr::LEN;
                let ipv4hdr_inner: *const Ipv4Hdr = unsafe { ptr_at(&ctx, inner_offset)? };
                let ipv4_inner = unsafe { &*ipv4hdr_inner };
                let proto = ipv4_inner.proto()?;
                let ihl: usize = ipv4_inner.ihl() as usize;
                let (src_port, dst_port, _) = match proto {
                    IpProto::Udp => Self::extract_for_udp(ctx, inner_offset + ihl)?,
                    IpProto::Tcp => Self::extract_for_tcp(ctx, inner_offset + ihl)?,
                    IpProto::Sctp => Self::extract_for_sctp(ctx, inner_offset + ihl)?,
                    _ => (0, 0, None),
                };

                Ok((src_port, dst_port, None))
            }

            _ => Ok((0, 0, None)),
        }
    }
}
