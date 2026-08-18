use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use sarena_infra::route::Route;

const IPV4_DEFAULT_ROUTE: IpNet = IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 0));
const IPV6_DEFAULT_ROUTE: IpNet = IpNet::V6(Ipv6Net::new_assert(Ipv6Addr::UNSPECIFIED, 0));

/// Routes to be installed in an endpoint's networking namespace: a host
/// route to the node's own IPv4 address, plus a default route via it.
pub fn ipv4_routes(ip: Ipv4Addr, link_mtu: u32) -> Vec<Route> {
    let ip = IpAddr::V4(ip);
    vec![
        Route {
            prefix: IpNet::from(ip),
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
            prefix: IpNet::from(ip),
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
