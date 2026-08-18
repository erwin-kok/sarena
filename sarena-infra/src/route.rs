use std::{
    cmp::Reverse,
    fmt,
    net::{IpAddr, Ipv4Addr},
};

use ipnet::{IpNet, Ipv4Net};
use netlink_packet_route::route::{RouteProtocol, RouteScope, RouteType};

/// The IPv4 "everything" prefix (`0.0.0.0/0`), built at compile time: if
/// the prefix were ever invalid, the crate would fail to build rather than
/// panicking at runtime.
const DEFAULT_PREFIX: IpNet = IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 0));

/// A single IP route: the local intent for what should be installed,
/// independent of which link or namespace it ends up added to (those are
/// supplied separately -- see `netlink_link`'s route-adding function).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Destination network this route matches.
    pub prefix: IpNet,
    /// Gateway to route through. `None` means an on-link/directly-connected
    /// route (no next hop) -- e.g. the subnet a link is itself attached to.
    pub nexthop: Option<IpAddr>,
    /// Preferred source address for packets sent via this route.
    pub local: Option<IpAddr>,
    /// The link this route is attached to, if known. Purely informational
    /// here -- actually adding a route always needs a real link index,
    /// supplied separately at the point it's installed.
    pub device: Option<String>,
    pub mtu: Option<u32>,
    pub priority: Option<u32>,
    pub proto: Option<RouteProtocol>,
    pub scope: Option<RouteScope>,
    pub table: Option<u32>,
    pub kind: Option<RouteType>,
}

impl Default for Route {
    /// Defaults to the IPv4 "everything" prefix (`0.0.0.0/0`) -- `IpNetwork`
    /// has no default of its own, so `prefix` has to be picked explicitly;
    /// callers building a route are expected to overwrite it.
    fn default() -> Self {
        Self {
            prefix: DEFAULT_PREFIX,
            nexthop: None,
            local: None,
            device: None,
            mtu: None,
            priority: None,
            proto: None,
            scope: None,
            table: None,
            kind: None,
        }
    }
}

impl fmt::Display for Route {
    /// A concise, loggable summary of this route -- attach it to a
    /// `tracing` call (`tracing::info!(%route, ...)`) the same way the
    /// reference implementation's `LogAttrs()` was meant to be attached to
    /// a logger.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "prefix={} nexthop={} local={} device={}",
            self.prefix,
            self.nexthop
                .map_or_else(|| "<nil>".to_string(), |ip| ip.to_string()),
            self.local
                .map_or_else(|| "<nil>".to_string(), |ip| ip.to_string()),
            self.device.as_deref().unwrap_or("<nil>"),
        )
    }
}

/// Sorts `routes` by prefix length, narrowest (most specific, i.e. longest
/// mask) first -- the order routing-table reconciliation needs, since a
/// more specific route must take precedence over a broader one that also
/// matches the same destination.
///
/// A plain `slice::sort_by_key` call over a `Reverse`-wrapped prefix length
/// is all this needs in Rust; there's no equivalent here of having to
/// define a dedicated sortable wrapper type (a `sort.Interface`
/// implementation in Go) purely to give the standard sort function
/// something to call back into.
pub fn sort_by_mask_narrowest_first(routes: &mut [Route]) {
    routes.sort_by_key(|route| Reverse(route.prefix.prefix_len()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(prefix: &str) -> Route {
        Route {
            prefix: prefix.parse().unwrap(),
            nexthop: None,
            local: None,
            device: None,
            mtu: None,
            priority: None,
            proto: None,
            scope: None,
            table: None,
            kind: None,
        }
    }

    #[test]
    fn sort_by_mask_narrowest_first_orders_longest_prefix_first() {
        let mut routes = vec![
            route("10.0.0.0/8"),
            route("10.1.2.3/32"),
            route("10.1.0.0/16"),
        ];

        sort_by_mask_narrowest_first(&mut routes);

        let prefixes: Vec<_> = routes.iter().map(|r| r.prefix.prefix_len()).collect();
        assert_eq!(prefixes, vec![32, 16, 8]);
    }
}
