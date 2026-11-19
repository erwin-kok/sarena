//! Picking the node's own address from the addresses configured on its
//! links, see [`NetworkProvisioner::first_global_ipv4`].

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{Ipv4Net, Ipv6Net};
use tracing::debug;

use crate::{
    AddressFamily, AddressScope, InfraError, Link, NetworkProvisioner, Res, ScopedAddress,
};

/// IPv4 ranges that are not publicly routable (IANA special-purpose
/// registry, RFC 6890 and successors).
const NON_PUBLIC_V4: &[Ipv4Net] = &[
    Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 8), // "this network"
    Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 0), 8), // RFC 1918
    Ipv4Net::new_assert(Ipv4Addr::new(100, 64, 0, 0), 10), // RFC 6598 shared/CGNAT
    Ipv4Net::new_assert(Ipv4Addr::new(127, 0, 0, 0), 8), // loopback
    Ipv4Net::new_assert(Ipv4Addr::new(169, 254, 0, 0), 16), // link-local
    Ipv4Net::new_assert(Ipv4Addr::new(172, 16, 0, 0), 12), // RFC 1918
    Ipv4Net::new_assert(Ipv4Addr::new(192, 0, 0, 0), 24), // IETF protocol assignments
    Ipv4Net::new_assert(Ipv4Addr::new(192, 0, 2, 0), 24), // TEST-NET-1
    Ipv4Net::new_assert(Ipv4Addr::new(192, 88, 99, 0), 24), // 6to4 relay anycast
    Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 0, 0), 16), // RFC 1918
    Ipv4Net::new_assert(Ipv4Addr::new(198, 18, 0, 0), 15), // benchmarking
    Ipv4Net::new_assert(Ipv4Addr::new(198, 51, 100, 0), 24), // TEST-NET-2
    Ipv4Net::new_assert(Ipv4Addr::new(203, 0, 113, 0), 24), // TEST-NET-3
    Ipv4Net::new_assert(Ipv4Addr::new(224, 0, 0, 0), 4), // multicast
    Ipv4Net::new_assert(Ipv4Addr::new(240, 0, 0, 0), 4), // reserved + broadcast
];

/// IPv6 ranges that are not publicly routable.
const NON_PUBLIC_V6: &[Ipv6Net] = &[
    Ipv6Net::new_assert(Ipv6Addr::UNSPECIFIED, 128),
    Ipv6Net::new_assert(Ipv6Addr::LOCALHOST, 128),
    Ipv6Net::new_assert(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96), // IPv4-mapped
    Ipv6Net::new_assert(Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48), // local-use NAT64
    Ipv6Net::new_assert(Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64),  // discard-only
    Ipv6Net::new_assert(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32), // documentation
    Ipv6Net::new_assert(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),  // unique local
    Ipv6Net::new_assert(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10), // link-local
    Ipv6Net::new_assert(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),  // multicast
];

/// Whether `ip` is publicly routable, i.e. outside every private, shared,
/// loopback, link-local, documentation, multicast and otherwise reserved range.
pub fn is_public_addr(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !NON_PUBLIC_V4.iter().any(|net| net.contains(ip)),
        IpAddr::V6(ip) => !NON_PUBLIC_V6.iter().any(|net| net.contains(ip)),
    }
}

/// Whether `ip` is a global unicast address: not unspecified, loopback,
/// multicast, link-local unicast or (IPv4) the limited broadcast address.
///
/// Unlike [`is_public_addr`] this holds for private addresses too (e.g.
/// `10.0.0.1`, `fd00::1`); it's the same check as Go's `IP.IsGlobalUnicast`.
pub fn is_global_unicast(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || ip.is_link_local()
                || ip.is_broadcast())
        }
        IpAddr::V6(ip) => {
            !(ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || ip.is_unicast_link_local())
        }
    }
}

/// Shared implementation of `first_global_ipv{4,6}`: first `ifname`'s own
/// addresses (without exclusions), then those of every link minus
/// [`NetworkProvisioner::excluded_ips`].
pub(crate) async fn first_global_addr<P: NetworkProvisioner + ?Sized>(
    network_provisioner: &P,
    ifname: Option<&str>,
    preferred: Option<IpAddr>,
    family: AddressFamily,
) -> Res<IpAddr> {
    if let Some(ifname) = ifname {
        match network_provisioner.get_link(ifname).await {
            Ok(link) => {
                let addrs = link.addresses(Some(family)).await?;
                if let Some(ip) = select_global_addr(&addrs, &[], preferred) {
                    return Ok(ip);
                }
                debug!(
                    ifname,
                    ?family,
                    "no global address on link, trying all links"
                );
            }
            Err(InfraError::LinkNotFound(_)) => {
                debug!(ifname, "link not found, trying all links");
            }
            Err(err) => return Err(err),
        }
    }

    let excluded = network_provisioner.excluded_ips().await?;
    let mut addrs = Vec::new();
    for link in network_provisioner.list_links().await? {
        // Same as `excluded_ips`: a link deleted after being listed is skipped.
        match link.addresses(Some(family)).await {
            Ok(link_addrs) => addrs.extend(link_addrs),
            Err(err) => debug!(
                ifname = link.ifname(),
                %err,
                "skipping link whose addresses can't be listed"
            ),
        }
    }

    select_global_addr(&addrs, &excluded, preferred).ok_or(InfraError::NoGlobalAddress(family))
}

/// Picks an address out of `addrs`, ignoring `excluded` ones:
///
/// 1. Only universe-scope addresses are considered; when there are none, the search is widened to
///    site and user-defined scopes.
/// 2. Public addresses win over private ones.
/// 3. Within the winning group `preferred` is returned if it's there, otherwise the lowest address,
///    so repeated calls give a stable result.
fn select_global_addr(
    addrs: &[ScopedAddress],
    excluded: &[IpAddr],
    preferred: Option<IpAddr>,
) -> Option<IpAddr> {
    [AddressScope::UNIVERSE, AddressScope::SITE]
        .into_iter()
        .find_map(|max_scope| {
            let (public, private): (Vec<IpAddr>, Vec<IpAddr>) = addrs
                .iter()
                .filter(|scoped| scoped.scope <= max_scope)
                .map(|scoped| scoped.addr.ip)
                .filter(|ip| !excluded.contains(ip))
                .partition(is_public_addr);
            let candidates = if public.is_empty() { private } else { public };

            preferred
                .filter(|preferred| candidates.contains(preferred))
                .or_else(|| candidates.into_iter().min())
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{InterfaceAddress, MockNetworkProvisioner, mock_link::MockLink};

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn scoped(addr: &str, scope: AddressScope) -> ScopedAddress {
        ScopedAddress {
            addr: addr.parse::<InterfaceAddress>().unwrap(),
            scope,
        }
    }

    fn universe(addr: &str) -> ScopedAddress {
        scoped(addr, AddressScope::UNIVERSE)
    }

    #[test]
    fn classifies_public_and_private_addresses() {
        for public in ["8.8.8.8", "1.1.1.1", "2001:4860::8888", "2a00:1450::1"] {
            assert!(is_public_addr(&ip(public)), "{public} should be public");
        }
        for private in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "192.0.2.1",
            "fd00::1",
            "fe80::1",
            "::1",
            "2001:db8::1",
        ] {
            assert!(
                !is_public_addr(&ip(private)),
                "{private} should not be public"
            );
        }
    }

    #[test]
    fn classifies_global_unicast_addresses() {
        for global in ["10.0.0.1", "8.8.8.8", "fd00::1", "2001:4860::1"] {
            assert!(
                is_global_unicast(&ip(global)),
                "{global} should be global unicast"
            );
        }
        for other in [
            "0.0.0.0",
            "127.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(
                !is_global_unicast(&ip(other)),
                "{other} should not be global unicast"
            );
        }
    }

    #[test]
    fn picks_lowest_address_for_stability() {
        let addrs = [universe("10.0.0.9/24"), universe("10.0.0.2/24")];
        assert_eq!(select_global_addr(&addrs, &[], None), Some(ip("10.0.0.2")));
    }

    #[test]
    fn public_wins_over_private_and_over_a_private_preferred() {
        let addrs = [universe("10.0.0.1/24"), universe("8.8.8.8/24")];
        assert_eq!(
            select_global_addr(&addrs, &[], Some(ip("10.0.0.1"))),
            Some(ip("8.8.8.8"))
        );
    }

    #[test]
    fn preferred_wins_within_its_group() {
        let addrs = [universe("10.0.0.1/24"), universe("10.0.0.5/24")];
        assert_eq!(
            select_global_addr(&addrs, &[], Some(ip("10.0.0.5"))),
            Some(ip("10.0.0.5"))
        );
        // Not configured: ignored.
        assert_eq!(
            select_global_addr(&addrs, &[], Some(ip("10.0.0.99"))),
            Some(ip("10.0.0.1"))
        );
    }

    #[test]
    fn falls_back_to_site_scope_but_never_link_or_host() {
        let addrs = [
            scoped("10.0.0.1/24", AddressScope::SITE),
            scoped("fe80::1/64", AddressScope::LINK),
        ];
        assert_eq!(select_global_addr(&addrs, &[], None), Some(ip("10.0.0.1")));

        let addrs = [
            scoped("fe80::1/64", AddressScope::LINK),
            scoped("127.0.0.1/8", AddressScope::HOST),
        ];
        assert_eq!(select_global_addr(&addrs, &[], None), None);
    }

    #[test]
    fn skips_excluded_addresses() {
        let addrs = [universe("10.0.0.1/24"), universe("10.0.0.2/24")];
        assert_eq!(
            select_global_addr(&addrs, &[ip("10.0.0.1")], None),
            Some(ip("10.0.0.2"))
        );
    }

    fn link(ifname: &str, oper_up: bool, addrs: &[&str]) -> MockLink {
        MockLink {
            ifname: ifname.to_owned(),
            oper_up,
            addr_calls: addrs.iter().map(|a| a.parse().unwrap()).collect(),
            addr_scopes: HashMap::new(),
            ..MockLink::default()
        }
    }

    #[tokio::test]
    async fn named_link_is_searched_first_without_exclusions() {
        let mut mock = MockNetworkProvisioner::default();
        mock.add_link(link("eth0", true, &["10.0.0.1/24"]));
        // Down, so its addresses are excluded from the all-links search --
        // but not when asked for by name.
        mock.add_link(link("eth1", false, &["10.0.1.1/24"]));

        let ip = mock.first_global_ipv4(Some("eth1"), None).await.unwrap();
        assert_eq!(ip, Ipv4Addr::new(10, 0, 1, 1));
    }

    #[tokio::test]
    async fn falls_back_to_all_links_when_named_link_is_missing_or_empty() {
        let mut mock = MockNetworkProvisioner::default();
        mock.add_link(link("eth0", true, &["10.0.0.1/24", "2001:4860::1/64"]));
        mock.add_link(link("eth1", true, &[]));
        mock.add_link(link("docker0", true, &["172.17.0.1/16"]));

        assert_eq!(
            mock.first_global_ipv4(Some("nope0"), None).await.unwrap(),
            Ipv4Addr::new(10, 0, 0, 1)
        );
        assert_eq!(
            mock.first_global_ipv4(Some("eth1"), None).await.unwrap(),
            Ipv4Addr::new(10, 0, 0, 1)
        );
        assert_eq!(
            mock.first_global_ipv6(None, None).await.unwrap(),
            "2001:4860::1".parse::<Ipv6Addr>().unwrap()
        );
    }

    #[tokio::test]
    async fn errors_when_nothing_usable_is_found() {
        let mut mock = MockNetworkProvisioner::default();
        mock.add_link(link("eth0", true, &["10.0.0.1/24"]));

        let err = mock.first_global_ipv6(None, None).await.unwrap_err();
        assert!(matches!(
            err,
            InfraError::NoGlobalAddress(AddressFamily::V6)
        ));
    }
}
