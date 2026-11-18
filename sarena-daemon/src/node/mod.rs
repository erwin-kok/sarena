use std::net::{Ipv4Addr, Ipv6Addr};

use ipnet::{Ipv4Net, Ipv6Net};

pub mod init;

/// Addressing of the local node: its own addresses and the ranges endpoint
/// addresses are allocated from.
#[derive(Debug, Clone, Default)]
pub struct LocalNode {
    /// IPv4 address of the node on the internal (pod-facing) network.
    ipv4_internal_address: Option<Ipv4Addr>,
    /// IPv4 address the node is reachable at from outside.
    ipv4_external_address: Option<Ipv4Addr>,
    /// IPv4 range endpoint addresses are allocated from.
    ipv4_allocation_range: Option<Ipv4Net>,
    /// IPv6 address of the node.
    ipv6_address: Option<Ipv6Addr>,
    /// IPv6 address of the node-side router endpoints use as their gateway.
    ipv6_router_address: Option<Ipv6Addr>,
    /// IPv6 range endpoint addresses are allocated from.
    ipv6_allocation_range: Option<Ipv6Net>,
}
