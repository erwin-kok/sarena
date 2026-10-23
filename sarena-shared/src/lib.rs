#![cfg_attr(not(feature = "std"), no_std)]
#![no_builtins]

mod endpoint;
mod metrics;

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub use endpoint::*;
pub use metrics::*;

pub const ENDPOINT_KEY_FAMILY_IPV4: u8 = 1;
pub const ENDPOINT_KEY_FAMILY_IPV6: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointKey {
    // represents both IPv6 and IPv4 (in the first four bytes)
    pub ip: [u8; 16],
    pub family: u8,
    pub pad: [u8; 3],
}

impl EndpointKey {
    /// Key for an IPv4 address in network byte order, as found in a packet header.
    #[inline]
    pub const fn from_ipv4_octets(octets: [u8; 4]) -> Self {
        let [a, b, c, d] = octets;
        Self {
            ip: [a, b, c, d, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            family: ENDPOINT_KEY_FAMILY_IPV4,
            pad: [0; 3],
        }
    }

    /// Key for an IPv6 address in network byte order, as found in a packet header.
    #[inline]
    pub const fn from_ipv6_octets(octets: [u8; 16]) -> Self {
        Self {
            ip: octets,
            family: ENDPOINT_KEY_FAMILY_IPV6,
            pad: [0; 3],
        }
    }
}

impl From<IpAddr> for EndpointKey {
    fn from(addr: IpAddr) -> Self {
        match addr {
            IpAddr::V4(ipv4_addr) => Self::from_ipv4_octets(ipv4_addr.octets()),
            IpAddr::V6(ipv6_addr) => Self::from_ipv6_octets(ipv6_addr.octets()),
        }
    }
}

impl TryFrom<EndpointKey> for IpAddr {
    type Error = &'static str;

    fn try_from(key: EndpointKey) -> Result<Self, Self::Error> {
        match key.family {
            ENDPOINT_KEY_FAMILY_IPV4 => {
                let [a, b, c, d, ..] = key.ip;

                Ok(IpAddr::V4(Ipv4Addr::new(a, b, c, d)))
            }
            ENDPOINT_KEY_FAMILY_IPV6 => Ok(IpAddr::V6(Ipv6Addr::from(key.ip))),
            _ => Err("Invalid endpoint key address family"),
        }
    }
}

#[cfg(feature = "std")]
mod pod_impls {
    use super::EndpointKey;

    unsafe impl aya::Pod for EndpointKey {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_layout_matches_the_ebpf_map() {
        assert_eq!(core::mem::size_of::<EndpointKey>(), 20);
        assert_eq!(core::mem::align_of::<EndpointKey>(), 1);
    }

    #[test]
    fn ipv4_is_stored_in_the_first_four_bytes() {
        let key = EndpointKey::from(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));
        assert_eq!(key.ip, [10, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(key.family, ENDPOINT_KEY_FAMILY_IPV4);
        assert_eq!(key.pad, [0; 3]);
    }

    #[test]
    fn round_trips_both_families() {
        for addr in [
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0x42)),
        ] {
            assert_eq!(IpAddr::try_from(EndpointKey::from(addr)), Ok(addr));
        }
    }

    #[test]
    fn rejects_an_unknown_family() {
        let mut key = EndpointKey::from_ipv4_octets([10, 0, 0, 1]);
        key.family = 0;
        assert!(IpAddr::try_from(key).is_err());
    }
}
