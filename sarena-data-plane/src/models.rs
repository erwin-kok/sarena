#![allow(dead_code)]

use std::{collections::BTreeMap, net::IpAddr, time::SystemTime};

use async_trait::async_trait;
use ipnet::IpNet;
use network_types::ip::IpProto;
use sarena_infra::MacAddress;

// ---------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------

/// A machine participating in the dataplane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub addresses: Vec<IpAddr>,
}

#[async_trait]
pub trait NodeTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `node.id`.
    async fn add_node(&self, node: Node) -> Result<(), Self::Error>;
    async fn remove_node(&self, id: NodeId) -> Result<(), Self::Error>;
    async fn list_nodes(&self) -> Result<Vec<Node>, Self::Error>;
}

// ---------------------------------------------------------------------
// Interface
// ---------------------------------------------------------------------

/// A dataplane attachment point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub id: InterfaceId,
    pub node: NodeId,
    pub kind: InterfaceKind,
    pub addresses: Vec<IpNet>,
    pub mac: Option<MacAddress>,
    pub mtu: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceKind {
    Physical,
    Veth,
    Bridge,
    Loopback,
    Tunnel,
    Virtual,
}

#[async_trait]
pub trait InterfaceTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `interface.id`.
    async fn add_interface(&self, interface: Interface) -> Result<(), Self::Error>;
    async fn remove_interface(&self, id: InterfaceId) -> Result<(), Self::Error>;
    async fn list_interfaces(&self) -> Result<Vec<Interface>, Self::Error>;
}

// ---------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------

/// Something that has network identity: a pod, VM, container, host-local
/// workload, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EndpointId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub id: EndpointId,
    pub node: NodeId,
    pub addresses: Vec<IpAddr>,
    pub identity: IdentityId,
    pub location: EndpointLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointLocation {
    Local { interface: InterfaceId },
    Remote,
}

#[async_trait]
pub trait EndpointTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `endpoint.id`.
    async fn add_endpoint(&self, endpoint: Endpoint) -> Result<(), Self::Error>;
    async fn remove_endpoint(&self, id: EndpointId) -> Result<(), Self::Error>;
    async fn list_endpoints(&self) -> Result<Vec<Endpoint>, Self::Error>;
}

// ---------------------------------------------------------------------
// Route
// ---------------------------------------------------------------------

/// L3 reachability. Keyed by `(destination, next_hop)` — a destination
/// can have more than one route for ECMP; each next-hop is added and
/// removed independently, same as every other resource in this crate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    pub destination: IpNet,
    pub next_hop: NextHop,
    /// Relative weight for ECMP, matching the kernel's own nexthop weight
    /// semantics (`ip route ... weight N`, 1-255). Equal weight across a
    /// destination's routes means equal-cost; a destination with a single
    /// route is just a normal, non-ECMP route regardless of the value.
    pub weight: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NextHop {
    Local(InterfaceId),
    Remote(NodeId),
    Gateway(IpAddr),
    Blackhole,
}

#[async_trait]
pub trait RouteTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `(route.destination, route.next_hop)`.
    async fn add_route(&self, route: Route) -> Result<(), Self::Error>;
    async fn remove_route(&self, destination: IpNet, next_hop: NextHop) -> Result<(), Self::Error>;
    async fn dump_routes(&self) -> Result<Vec<Route>, Self::Error>;
}

// ---------------------------------------------------------------------
// Identity (used by Endpoint and Policy)
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IdentityId(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: IdentityId,
    pub attributes: IdentityAttributes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityAttributes {
    pub labels: BTreeMap<String, String>,
}

#[async_trait]
pub trait IdentityTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `identity.id`. Unlike Node/Endpoint, this id is expected
    /// to be assigned by something outside this crate that agrees on
    /// identities across the whole cluster — a locally-invented id here
    /// would mean the same number meaning different things on different
    /// nodes, which breaks any policy decision made from a remote
    /// endpoint's identity alone.
    async fn add_identity(&self, identity: Identity) -> Result<(), Self::Error>;
    async fn remove_identity(&self, id: IdentityId) -> Result<(), Self::Error>;
    async fn list_identities(&self) -> Result<Vec<Identity>, Self::Error>;
}

// ---------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------

/// Caller-supplied id, since nothing about a policy's content is
/// guaranteed stable/unique across updates. Governs traffic for endpoints
/// matching `subject`, to/from endpoints matching `peer`, in `direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PolicyId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub id: PolicyId,
    pub subject: IdentitySelector,
    pub direction: Direction,
    pub peer: IdentitySelector,
    pub ports: Vec<PortRule>,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentitySelector {
    Identity(IdentityId),
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ingress,
    Egress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortRule {
    pub protocol: IpProto,
    pub port: Option<u16>, // None = all ports for this protocol
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

#[async_trait]
pub trait PolicyTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `policy.id`.
    async fn add_policy(&self, policy: Policy) -> Result<(), Self::Error>;
    async fn remove_policy(&self, id: PolicyId) -> Result<(), Self::Error>;
    async fn list_policies(&self) -> Result<Vec<Policy>, Self::Error>;
}

// ---------------------------------------------------------------------
// Tunnel
// ---------------------------------------------------------------------

/// A kernel VXLAN path between two nodes. No synthetic id — the node pair
/// is already unique. A backend implements this by creating/updating a
/// kernel vxlan interface and its FDB entry via netlink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub local_node: NodeId,
    pub remote_node: NodeId,
    pub kind: TunnelKind,
    pub local_address: IpAddr,
    pub remote_address: IpAddr,
    pub mtu: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelKind {
    Vxlan { vni: u32 },
    // Geneve(GeneveConfig),
    // IpIp(IpIpConfig),
    // Gre(GreConfig),
}

#[async_trait]
pub trait TunnelTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `(tunnel.local_node, tunnel.remote_node)`.
    async fn add_tunnel(&self, tunnel: Tunnel) -> Result<(), Self::Error>;
    async fn remove_tunnel(
        &self,
        local_node: NodeId,
        remote_node: NodeId,
    ) -> Result<(), Self::Error>;
    async fn list_tunnels(&self) -> Result<Vec<Tunnel>, Self::Error>;
}

// ---------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------

/// Keyed by `frontend` — two services can't share a listening tuple, so
/// (address, port, protocol) is already unique.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub frontend: ServiceFrontend,
    pub backends: Vec<ServiceBackend>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ServiceFrontend {
    pub address: IpAddr,
    pub port: u16,
    pub protocol: IpProto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceBackend {
    pub endpoint: EndpointId,
    pub port: u16,
    pub nat: Nat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nat {
    Source { address: IpAddr },
    Destination { address: IpAddr, port: Option<u16> },
    Masquerade,
}

#[async_trait]
pub trait ServiceTable: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    /// Upsert by `service.frontend`.
    async fn add_service(&self, service: Service) -> Result<(), Self::Error>;
    async fn remove_service(&self, frontend: ServiceFrontend) -> Result<(), Self::Error>;
    async fn list_services(&self) -> Result<Vec<Service>, Self::Error>;
}

// ---------------------------------------------------------------------
// Conntrack
// ---------------------------------------------------------------------

/// Conntrack entries are created and expired by the dataplane itself in
/// response to traffic, never through an add/remove pair — so there's no
/// upsert question here. `FlowTuple` is the natural lookup key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConntrackEntry {
    pub original: FlowTuple,
    pub reply: FlowTuple,
    pub state: ConnectionState,
    pub last_seen: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlowTuple {
    pub src: IpAddr,
    pub src_port: u16,
    pub dst: IpAddr,
    pub dst_port: u16,
    pub protocol: IpProto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    New,
    Established,
    Closing,
    Closed,
}

/// Conntrack is observed, not declared — the dataplane owns creation and
/// expiry in response to real traffic.
#[async_trait]
pub trait ConntrackView: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    async fn lookup_conntrack(
        &self,
        tuple: FlowTuple,
    ) -> Result<Option<ConntrackEntry>, Self::Error>;
    async fn dump_conntrack(&self) -> Result<Vec<ConntrackEntry>, Self::Error>;
    async fn flush_conntrack(&self, endpoint: EndpointId) -> Result<(), Self::Error>;
}

// pub trait Forwarder {
//     fn process(&self, packet: Packet) -> ForwardingDecision;
// }

// pub enum ForwardingDecision {
//     Local(InterfaceId),
//     Forward(NextHop),
//     Tunnel(TunnelId),
//     Drop(DropReason),
// }

#[async_trait]
pub trait Dataplane:
    NodeTable<Error = Self::Err>
    + InterfaceTable<Error = Self::Err>
    + EndpointTable<Error = Self::Err>
    + RouteTable<Error = Self::Err>
    + IdentityTable<Error = Self::Err>
    + TunnelTable<Error = Self::Err>
    + ServiceTable<Error = Self::Err>
    + PolicyTable<Error = Self::Err>
    + ConntrackView<Error = Self::Err>
{
    type Err: std::error::Error + Send + Sync + 'static;
}
