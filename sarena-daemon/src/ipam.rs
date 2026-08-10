use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnetwork::{IpNetwork, Ipv4Network, Ipv6Network};
use sarena_infra::route::Route;

const IPV4_DEFAULT_ROUTE: IpNetwork =
    IpNetwork::V4(match Ipv4Network::new_checked(Ipv4Addr::UNSPECIFIED, 0) {
        Some(network) => network,
        None => unreachable!(),
    });

const IPV6_DEFAULT_ROUTE: IpNetwork =
    IpNetwork::V6(match Ipv6Network::new_checked(Ipv6Addr::UNSPECIFIED, 0) {
        Some(network) => network,
        None => unreachable!(),
    });

/// Routes to be installed in an endpoint's networking namespace: a host
/// route to the node's own IPv4 address, plus a default route via it.
pub fn ipv4_routes(ip: Ipv4Addr, link_mtu: u32) -> Vec<Route> {
    let ip = IpAddr::V4(ip);
    vec![
        Route {
            prefix: IpNetwork::from(ip),
            ..Default::default()
        },
        Route {
            prefix: IPV4_DEFAULT_ROUTE,
            nexthop: Some(ip),
            mtu: Some(link_mtu),
            ..Default::default()
        },
    ]
}

/// Routes to be installed in an endpoint's networking namespace: a host
/// route to the node's own IPv6 address, plus a default route via it.
pub fn ipv6_routes(ip: Ipv6Addr, link_mtu: u32) -> Vec<Route> {
    let ip = IpAddr::V6(ip);
    vec![
        Route {
            prefix: IpNetwork::from(ip),
            ..Default::default()
        },
        Route {
            prefix: IPV6_DEFAULT_ROUTE,
            nexthop: Some(ip),
            mtu: Some(link_mtu),
            ..Default::default()
        },
    ]
}
