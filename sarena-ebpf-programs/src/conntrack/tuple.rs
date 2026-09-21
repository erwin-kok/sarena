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
    pub is_related: bool,
}

impl ConnTrackTuple {
    #[inline(always)]
    pub fn new(ctx: &TcContext) -> Res<Self> {
        let ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, EthHdr::LEN)? };
        let ipv4 = unsafe { &*ipv4hdr };

        let proto = ipv4.proto()?;
        let l4_off: usize = EthHdr::LEN + ipv4.ihl() as usize;

        let mut t = Self {
            src_addr: Ipv4Key::from_octets(ipv4.src_addr),
            dst_addr: Ipv4Key::from_octets(ipv4.dst_addr),
            src_port: 0,
            dst_port: 0,
            proto: proto as u8,
            tcp_flags: None,
            is_related: false,
        };

        match proto {
            IpProto::Udp => {
                let (s, d, f) = Self::extract_for_udp(ctx, l4_off)?;
                t.src_port = s;
                t.dst_port = d;
                t.tcp_flags = f;
            }
            IpProto::Tcp => {
                let (s, d, f) = Self::extract_for_tcp(ctx, l4_off)?;
                t.src_port = s;
                t.dst_port = d;
                t.tcp_flags = f;
            }
            IpProto::Sctp => {
                let (s, d, f) = Self::extract_for_sctp(ctx, l4_off)?;
                t.src_port = s;
                t.dst_port = d;
                t.tcp_flags = f;
            }
            IpProto::Icmp => t.fill_icmp(ctx, l4_off)?,
            _ => return Err(UnsupportedProtocol(proto as u8)),
        };

        Ok(t)
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
    fn fill_icmp(&mut self, ctx: &TcContext, offset: usize) -> Res<()> {
        let icmphdr: *const Icmpv4Hdr = unsafe { ptr_at(&ctx, offset)? };
        let icmp = unsafe { &*icmphdr };
        let data = icmp.data()?;
        match icmp.type_ {
            0 | 8 => {
                // Echo Reply / Echo Request: track by identifier, like a pseudo-port.
                let id = u16::from_be_bytes([icmp.data[0], icmp.data[1]]);
                self.src_port = id;
                self.dst_port = id;
            }

            3 | 11 | 12 => {
                // Destination Unreachable / Time Exceeded / Parameter Problem: the
                // embedded packet, not this ICMP message, is what needs to match
                // a live flow -- overwrite every identity field, addresses and
                // protocol included, with its values.
                let inner_off = offset + Icmpv4Hdr::LEN;
                let inner_ipv4hdr: *const Ipv4Hdr = unsafe { ptr_at(&ctx, inner_off)? };
                let inner_ipv4 = unsafe { &*inner_ipv4hdr };
                let inner_proto = inner_ipv4.proto()?;

                self.src_addr = Ipv4Key::from_octets(inner_ipv4.src_addr);
                self.dst_addr = Ipv4Key::from_octets(inner_ipv4.dst_addr);
                self.proto = inner_proto as u8;
                self.is_related = true;

                let inner_l4_off = inner_off + inner_ipv4.ihl() as usize;
                let (s, d, _) = match inner_proto {
                    IpProto::Udp => Self::extract_for_udp(ctx, inner_l4_off)?,
                    IpProto::Tcp => Self::extract_for_tcp(ctx, inner_l4_off)?,
                    IpProto::Sctp => Self::extract_for_sctp(ctx, inner_l4_off)?,
                    _ => (0, 0, None),
                };
                self.src_port = s;
                self.dst_port = d;
            }

            t => return Err(UnsupportedProtocol(t)),
        };
        Ok(())
    }
}
