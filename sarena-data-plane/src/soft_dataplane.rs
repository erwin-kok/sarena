#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::SystemTime,
};

use async_trait::async_trait;
use ipnet::IpNet;
use network_types::ip::IpProto;
use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock, mpsc},
};

use crate::models::{
    ConnectionState, ConntrackEntry, ConntrackView, Dataplane, Endpoint, EndpointId, EndpointTable,
    FlowTuple, Identity, IdentityId, IdentityTable, Interface, InterfaceId, InterfaceTable, Nat,
    NextHop, Node, NodeId, NodeTable, Policy, PolicyId, PolicyTable, Route, RouteTable, Service,
    ServiceBackend, ServiceFrontend, ServiceTable, Tunnel, TunnelKind, TunnelTable,
};

#[async_trait]
pub trait RawInterface: Send + Sync {
    async fn recv(&self) -> std::io::Result<Vec<u8>>;
    async fn send(&self, frame: &[u8]) -> std::io::Result<()>;
}

pub struct ChannelInterface {
    rx: Mutex<mpsc::Receiver<Vec<u8>>>,
    tx: mpsc::Sender<Vec<u8>>,
}

impl ChannelInterface {
    pub fn new_pair() -> (Arc<Self>, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (in_tx, in_rx) = mpsc::channel(64);
        let (out_tx, out_rx) = mpsc::channel(64);
        let iface = Arc::new(Self {
            rx: Mutex::new(in_rx),
            tx: out_tx,
        });
        (iface, in_tx, out_rx)
    }
}

#[async_trait]
impl RawInterface for ChannelInterface {
    async fn recv(&self) -> std::io::Result<Vec<u8>> {
        self.rx
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
    }

    async fn send(&self, frame: &[u8]) -> std::io::Result<()> {
        self.tx
            .send(frame.to_vec())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed"))
    }
}

#[derive(Default)]
struct State {
    nodes: HashMap<NodeId, Node>,
    interfaces: HashMap<InterfaceId, Interface>,
    endpoints: HashMap<EndpointId, Endpoint>,
    routes: HashMap<IpNet, Vec<Route>>,
    identities: HashMap<IdentityId, Identity>,
    tunnels: HashMap<(NodeId, NodeId), Tunnel>,
    services: HashMap<ServiceFrontend, Service>,
    policies: HashMap<PolicyId, Policy>,
    conntrack: HashMap<FlowTuple, ConntrackEntry>,
    conntrack_reply_index: HashMap<FlowTuple, FlowTuple>,
}

enum Decision {
    Deliver {
        interface: InterfaceId,
        frame: Vec<u8>,
    },
    Encapsulate {
        remote_addr: IpAddr,
        vni: u32,
        frame: Vec<u8>,
    },
    Drop,
}

pub struct SoftDataplane {
    local_node: NodeId,
    vxlan_port: u16,
    state: RwLock<State>,
    interfaces_io: RwLock<HashMap<InterfaceId, Arc<dyn RawInterface>>>,
    vxlan_socket: RwLock<Option<Arc<UdpSocket>>>,
}

impl SoftDataplane {
    pub fn new(local_node: NodeId, vxlan_port: u16) -> Self {
        Self {
            local_node,
            vxlan_port,
            state: RwLock::new(State::default()),
            interfaces_io: RwLock::new(HashMap::new()),
            vxlan_socket: RwLock::new(None),
        }
    }

    pub async fn attach_interface(&self, id: InterfaceId, io: Arc<dyn RawInterface>) {
        self.interfaces_io.write().await.insert(id, io);
    }

    async fn handle_frame(&self, frame: Vec<u8>) {
        let decision = {
            let mut state = self.state.write().await;
            self.decide(&mut state, frame)
        };
        match decision {
            Decision::Deliver { interface, frame } => {
                let io = self.interfaces_io.read().await.get(&interface).cloned();
                if let Some(io) = io {
                    let _ = io.send(&frame).await;
                }
            }
            Decision::Encapsulate {
                remote_addr,
                vni,
                frame,
            } => {
                let socket = self.vxlan_socket.read().await.clone();
                if let Some(socket) = socket {
                    let mut datagram = build_vxlan_header(vni);
                    datagram.extend_from_slice(&frame);
                    let addr = SocketAddr::new(remote_addr, self.vxlan_port);
                    let _ = socket.send_to(&datagram, addr).await;
                }
            }
            Decision::Drop => {}
        }
    }

    fn decide(&self, state: &mut State, mut frame: Vec<u8>) -> Decision {
        let Some(ip) = parse_ipv4(&frame) else {
            return Decision::Drop;
        };
        let Some(protocol) = ip_proto_from_byte(ip.protocol) else {
            return Decision::Drop;
        };
        let l4_off = ETH_HDR_LEN + ip.ihl;
        let (src_port, dst_port) = if matches!(protocol, IpProto::Tcp | IpProto::Udp) {
            parse_ports(&frame, l4_off).unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        let wire_tuple = FlowTuple {
            src: IpAddr::V4(ip.src),
            src_port,
            dst: IpAddr::V4(ip.dst),
            dst_port,
            protocol,
        };

        // 1. Return traffic for a flow we already NAT'd?
        if let Some(original) = state.conntrack_reply_index.get(&wire_tuple).copied() {
            if let Some(entry) = state.conntrack.get_mut(&original) {
                entry.last_seen = SystemTime::now();
                if entry.state == ConnectionState::New {
                    entry.state = ConnectionState::Established;
                }
                if let IpAddr::V4(frontend_addr) = entry.original.dst {
                    rewrite_ipv4(
                        &mut frame,
                        ip.ihl,
                        Some(frontend_addr),
                        None,
                        Some(entry.original.dst_port),
                        None,
                    );
                }
            }
            return self.forward(state, &frame);
        }

        // 2. New traffic to a known service frontend?
        let frontend = ServiceFrontend {
            address: IpAddr::V4(ip.dst),
            port: dst_port,
            protocol,
        };
        if let Some(service) = state.services.get(&frontend).cloned() {
            let Some(backend) = select_backend(&service.backends, &wire_tuple) else {
                return Decision::Drop;
            };
            let Nat::Destination {
                address: IpAddr::V4(backend_addr),
                port,
            } = backend.nat
            else {
                return Decision::Drop; // IPv6 backend or non-Destination Nat: not handled yet
            };
            let backend_port = port.unwrap_or(backend.port);
            let reply = FlowTuple {
                src: IpAddr::V4(backend_addr),
                src_port: backend_port,
                dst: wire_tuple.src,
                dst_port: wire_tuple.src_port,
                protocol,
            };
            state.conntrack.insert(
                wire_tuple,
                ConntrackEntry {
                    original: wire_tuple,
                    reply,
                    state: ConnectionState::New,
                    last_seen: SystemTime::now(),
                },
            );
            state.conntrack_reply_index.insert(reply, wire_tuple);
            rewrite_ipv4(
                &mut frame,
                ip.ihl,
                None,
                Some(backend_addr),
                None,
                Some(backend_port),
            );
        }

        self.forward(state, &frame)
    }

    /// Plain FIB lookup and next-hop dispatch — no NAT, no conntrack.
    fn forward(&self, state: &State, frame: &[u8]) -> Decision {
        let Some(ip) = parse_ipv4(frame) else {
            return Decision::Drop;
        };
        let l4_off = ETH_HDR_LEN + ip.ihl;
        let protocol = ip_proto_from_byte(ip.protocol).unwrap_or(IpProto::Icmp);
        let (src_port, dst_port) = if matches!(protocol, IpProto::Tcp | IpProto::Udp) {
            parse_ports(frame, l4_off).unwrap_or((0, 0))
        } else {
            (0, 0)
        };
        let flow = FlowTuple {
            src: IpAddr::V4(ip.src),
            src_port,
            dst: IpAddr::V4(ip.dst),
            dst_port,
            protocol,
        };

        let Some(route) = lookup_route(&state.routes, IpAddr::V4(ip.dst), &flow) else {
            return Decision::Drop;
        };
        match route.next_hop {
            NextHop::Local(iface_id) => {
                let mut out = frame.to_vec();
                if let Some(iface) = state.interfaces.get(&iface_id)
                    && let Some(mac) = iface.mac
                {
                    out[0..6].copy_from_slice(&mac.0);
                }
                Decision::Deliver {
                    interface: iface_id,
                    frame: out,
                }
            }
            NextHop::Remote(node_id) => match state.tunnels.get(&(self.local_node, node_id)) {
                Some(tunnel) => {
                    let vni = match tunnel.kind {
                        TunnelKind::Vxlan { vni } => vni,
                    };
                    Decision::Encapsulate {
                        remote_addr: tunnel.remote_address,
                        vni,
                        frame: frame.to_vec(),
                    }
                }
                None => Decision::Drop,
            },
            NextHop::Gateway(_) | NextHop::Blackhole => Decision::Drop,
        }
    }
}

const VXLAN_HDR_LEN: usize = 8;

fn build_vxlan_header(vni: u32) -> Vec<u8> {
    let mut hdr = vec![0u8; VXLAN_HDR_LEN];
    hdr[0] = 0x08; // "I" flag: VNI is valid
    let vni_bytes = vni.to_be_bytes();
    hdr[4] = vni_bytes[1];
    hdr[5] = vni_bytes[2];
    hdr[6] = vni_bytes[3];
    hdr
}

fn select_route<'a>(routes: &'a [Route], flow: &FlowTuple) -> Option<&'a Route> {
    if routes.is_empty() {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    flow.hash(&mut hasher);
    let total_weight: u32 = routes.iter().map(|r| u32::from(r.weight.max(1))).sum();
    let mut target = (hasher.finish() % u64::from(total_weight)) as u32;
    for r in routes {
        let w = u32::from(r.weight.max(1));
        if target < w {
            return Some(r);
        }
        target -= w;
    }
    routes.last()
}

fn lookup_route(
    routes: &HashMap<IpNet, Vec<Route>>,
    dst: IpAddr,
    flow: &FlowTuple,
) -> Option<Route> {
    let mut best: Option<(u8, &Vec<Route>)> = None;
    for (net, group) in routes {
        if net.contains(&dst) {
            let len = net.prefix_len();
            if best.is_none_or(|(b, _)| len > b) {
                best = Some((len, group));
            }
        }
    }
    select_route(best?.1, flow).cloned()
}

fn select_backend<'a>(
    backends: &'a [ServiceBackend],
    flow: &FlowTuple,
) -> Option<&'a ServiceBackend> {
    if backends.is_empty() {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    flow.hash(&mut hasher);
    backends.get((hasher.finish() as usize) % backends.len())
}

const ETH_HDR_LEN: usize = 14;
const ETHERTYPE_IPV4: u16 = 0x0800;

struct Ipv4Info {
    ihl: usize,
    protocol: u8,
    src: Ipv4Addr,
    dst: Ipv4Addr,
}

fn parse_ipv4(frame: &[u8]) -> Option<Ipv4Info> {
    if frame.len() < ETH_HDR_LEN + 20 {
        return None;
    }
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    if ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = &frame[ETH_HDR_LEN..];
    if ip[0] >> 4 != 4 {
        return None;
    }
    let ihl = ((ip[0] & 0x0F) as usize) * 4;
    if ip.len() < ihl {
        return None;
    }
    Some(Ipv4Info {
        ihl,
        protocol: ip[9],
        src: Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]),
        dst: Ipv4Addr::new(ip[16], ip[17], ip[18], ip[19]),
    })
}

fn parse_ports(frame: &[u8], l4_offset: usize) -> Option<(u16, u16)> {
    if frame.len() < l4_offset + 4 {
        return None;
    }
    Some((
        u16::from_be_bytes([frame[l4_offset], frame[l4_offset + 1]]),
        u16::from_be_bytes([frame[l4_offset + 2], frame[l4_offset + 3]]),
    ))
}

/// Maps a raw IP protocol number to `IpProto`. Only the values this crate
/// actually handles; extend as needed. If `network_types` exposes an
/// official `TryFrom<u8>` for `IpProto`, prefer that over this.
fn ip_proto_from_byte(byte: u8) -> Option<IpProto> {
    match byte {
        1 => Some(IpProto::Icmp),
        6 => Some(IpProto::Tcp),
        17 => Some(IpProto::Udp),
        _ => None,
    }
}

/// RFC 1071 internet checksum.
fn internet_checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut chunks = bytes.chunks_exact(2);
    for chunk in &mut chunks {
        sum += u32::from(u16::from_be_bytes([chunk[0], chunk[1]]));
    }
    if let [last] = chunks.remainder() {
        sum += u32::from(*last) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Rewrites the IPv4 src/dst address and/or L4 src/dst port in place and
/// recomputes both the IP and L4 checksums from scratch. `None` fields
/// are left untouched. Only TCP and UDP get an L4 checksum/port rewrite;
/// other protocols only get the IP-level rewrite.
fn rewrite_ipv4(
    frame: &mut [u8],
    ihl: usize,
    new_src_addr: Option<Ipv4Addr>,
    new_dst_addr: Option<Ipv4Addr>,
    new_src_port: Option<u16>,
    new_dst_port: Option<u16>,
) {
    let ip_off = ETH_HDR_LEN;
    if let Some(addr) = new_src_addr {
        frame[ip_off + 12..ip_off + 16].copy_from_slice(&addr.octets());
    }
    if let Some(addr) = new_dst_addr {
        frame[ip_off + 16..ip_off + 20].copy_from_slice(&addr.octets());
    }
    frame[ip_off + 10] = 0;
    frame[ip_off + 11] = 0;
    let ip_csum = internet_checksum(&frame[ip_off..ip_off + ihl]);
    frame[ip_off + 10..ip_off + 12].copy_from_slice(&ip_csum.to_be_bytes());

    let protocol = frame[ip_off + 9];
    let l4_off = ip_off + ihl;
    if protocol == 6 || protocol == 17 {
        if let Some(port) = new_src_port {
            frame[l4_off..l4_off + 2].copy_from_slice(&port.to_be_bytes());
        }
        if let Some(port) = new_dst_port {
            frame[l4_off + 2..l4_off + 4].copy_from_slice(&port.to_be_bytes());
        }
    }
    recompute_l4_checksum(frame, ip_off, ihl, protocol);
}

fn recompute_l4_checksum(frame: &mut [u8], ip_off: usize, ihl: usize, protocol: u8) {
    let total_len = u16::from_be_bytes([frame[ip_off + 2], frame[ip_off + 3]]) as usize;
    let l4_off = ip_off + ihl;
    if total_len < ihl || l4_off + (total_len - ihl) > frame.len() {
        return;
    }
    let l4_len = total_len - ihl;

    let mut pseudo = Vec::with_capacity(12 + l4_len);
    pseudo.extend_from_slice(&frame[ip_off + 12..ip_off + 16]);
    pseudo.extend_from_slice(&frame[ip_off + 16..ip_off + 20]);
    pseudo.push(0);
    pseudo.push(protocol);
    pseudo.extend_from_slice(&(l4_len as u16).to_be_bytes());

    match protocol {
        6 => {
            frame[l4_off + 16] = 0;
            frame[l4_off + 17] = 0;
            pseudo.extend_from_slice(&frame[l4_off..l4_off + l4_len]);
            let csum = internet_checksum(&pseudo);
            frame[l4_off + 16..l4_off + 18].copy_from_slice(&csum.to_be_bytes());
        }
        17 => {
            frame[l4_off + 6] = 0;
            frame[l4_off + 7] = 0;
            pseudo.extend_from_slice(&frame[l4_off..l4_off + l4_len]);
            let mut csum = internet_checksum(&pseudo);
            if csum == 0 {
                csum = 0xFFFF; // 0 means "no checksum" for UDP/IPv4
            }
            frame[l4_off + 6..l4_off + 8].copy_from_slice(&csum.to_be_bytes());
        }
        _ => {}
    }
}

#[async_trait]
impl NodeTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_node(&self, node: Node) -> Result<(), Self::Error> {
        self.state.write().await.nodes.insert(node.id, node);
        Ok(())
    }
    async fn remove_node(&self, id: NodeId) -> Result<(), Self::Error> {
        self.state.write().await.nodes.remove(&id);
        Ok(())
    }
    async fn list_nodes(&self) -> Result<Vec<Node>, Self::Error> {
        Ok(self.state.read().await.nodes.values().cloned().collect())
    }
}

#[async_trait]
impl InterfaceTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_interface(&self, interface: Interface) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .interfaces
            .insert(interface.id, interface);
        Ok(())
    }
    async fn remove_interface(&self, id: InterfaceId) -> Result<(), Self::Error> {
        self.state.write().await.interfaces.remove(&id);
        Ok(())
    }
    async fn list_interfaces(&self) -> Result<Vec<Interface>, Self::Error> {
        Ok(self
            .state
            .read()
            .await
            .interfaces
            .values()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl EndpointTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_endpoint(&self, endpoint: Endpoint) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .endpoints
            .insert(endpoint.id, endpoint);
        Ok(())
    }
    async fn remove_endpoint(&self, id: EndpointId) -> Result<(), Self::Error> {
        self.state.write().await.endpoints.remove(&id);
        Ok(())
    }
    async fn list_endpoints(&self) -> Result<Vec<Endpoint>, Self::Error> {
        Ok(self
            .state
            .read()
            .await
            .endpoints
            .values()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl RouteTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_route(&self, route: Route) -> Result<(), Self::Error> {
        let mut state = self.state.write().await;
        let group = state.routes.entry(route.destination).or_default();
        match group.iter_mut().find(|r| r.next_hop == route.next_hop) {
            Some(existing) => *existing = route,
            None => group.push(route),
        }
        Ok(())
    }
    async fn remove_route(&self, destination: IpNet, next_hop: NextHop) -> Result<(), Self::Error> {
        let mut state = self.state.write().await;
        if let Some(group) = state.routes.get_mut(&destination) {
            group.retain(|r| r.next_hop != next_hop);
            if group.is_empty() {
                state.routes.remove(&destination);
            }
        }
        Ok(())
    }
    async fn dump_routes(&self) -> Result<Vec<Route>, Self::Error> {
        Ok(self
            .state
            .read()
            .await
            .routes
            .values()
            .flatten()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl IdentityTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_identity(&self, identity: Identity) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .identities
            .insert(identity.id, identity);
        Ok(())
    }
    async fn remove_identity(&self, id: IdentityId) -> Result<(), Self::Error> {
        self.state.write().await.identities.remove(&id);
        Ok(())
    }
    async fn list_identities(&self) -> Result<Vec<Identity>, Self::Error> {
        Ok(self
            .state
            .read()
            .await
            .identities
            .values()
            .cloned()
            .collect())
    }
}

#[async_trait]
impl TunnelTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_tunnel(&self, tunnel: Tunnel) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .tunnels
            .insert((tunnel.local_node, tunnel.remote_node), tunnel);
        Ok(())
    }
    async fn remove_tunnel(
        &self,
        local_node: NodeId,
        remote_node: NodeId,
    ) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .tunnels
            .remove(&(local_node, remote_node));
        Ok(())
    }
    async fn list_tunnels(&self) -> Result<Vec<Tunnel>, Self::Error> {
        Ok(self.state.read().await.tunnels.values().cloned().collect())
    }
}

#[async_trait]
impl ServiceTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_service(&self, service: Service) -> Result<(), Self::Error> {
        self.state
            .write()
            .await
            .services
            .insert(service.frontend, service);
        Ok(())
    }
    async fn remove_service(&self, frontend: ServiceFrontend) -> Result<(), Self::Error> {
        self.state.write().await.services.remove(&frontend);
        Ok(())
    }
    async fn list_services(&self) -> Result<Vec<Service>, Self::Error> {
        Ok(self.state.read().await.services.values().cloned().collect())
    }
}

#[async_trait]
impl PolicyTable for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn add_policy(&self, policy: Policy) -> Result<(), Self::Error> {
        self.state.write().await.policies.insert(policy.id, policy);
        Ok(())
    }
    async fn remove_policy(&self, id: PolicyId) -> Result<(), Self::Error> {
        self.state.write().await.policies.remove(&id);
        Ok(())
    }
    async fn list_policies(&self) -> Result<Vec<Policy>, Self::Error> {
        Ok(self.state.read().await.policies.values().cloned().collect())
    }
}

#[async_trait]
impl ConntrackView for SoftDataplane {
    type Error = std::convert::Infallible;
    async fn lookup_conntrack(
        &self,
        tuple: FlowTuple,
    ) -> Result<Option<ConntrackEntry>, Self::Error> {
        Ok(self.state.read().await.conntrack.get(&tuple).cloned())
    }
    async fn dump_conntrack(&self) -> Result<Vec<ConntrackEntry>, Self::Error> {
        Ok(self
            .state
            .read()
            .await
            .conntrack
            .values()
            .cloned()
            .collect())
    }
    async fn flush_conntrack(&self, endpoint: EndpointId) -> Result<(), Self::Error> {
        let mut state = self.state.write().await;
        let Some(ep) = state.endpoints.get(&endpoint).cloned() else {
            return Ok(());
        };
        let addrs: HashSet<IpAddr> = ep.addresses.into_iter().collect();
        let stale: Vec<FlowTuple> = state
            .conntrack
            .values()
            .filter(|e| addrs.contains(&e.original.src) || addrs.contains(&e.reply.src))
            .map(|e| e.original)
            .collect();
        for key in stale {
            if let Some(entry) = state.conntrack.remove(&key) {
                state.conntrack_reply_index.remove(&entry.reply);
            }
        }
        Ok(())
    }
}

impl Dataplane for SoftDataplane {
    type Err = std::convert::Infallible;
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use network_types::ip::IpProto;
    use sarena_infra::MacAddress;

    use crate::{
        models::{
            Endpoint, EndpointId, EndpointLocation, EndpointTable, IdentityId, Interface,
            InterfaceId, InterfaceKind, InterfaceTable, Nat, NextHop, NodeId, Route, RouteTable,
            Service, ServiceBackend, ServiceFrontend, ServiceTable,
        },
        soft_dataplane::{ChannelInterface, SoftDataplane, parse_ipv4},
    };

    #[tokio::test]
    async fn local_delivery_rewrites_dst_mac() {
        let dp = SoftDataplane::new(NodeId(1), 4789);
        let iface_b = InterfaceId(11);
        let mac_b = MacAddress::generate_rand();

        dp.add_interface(Interface {
            id: iface_b,
            node: NodeId(1),
            kind: InterfaceKind::Veth,
            addresses: vec![],
            mac: Some(mac_b),
            mtu: None,
        })
        .await
        .unwrap();

        dp.add_route(Route {
            destination: "10.0.0.2/32".parse().unwrap(),
            next_hop: NextHop::Local(iface_b),
            weight: 1,
        })
        .await
        .unwrap();

        let (io_b, _in_tx_b, mut out_rx_b) = ChannelInterface::new_pair();
        dp.attach_interface(iface_b, io_b).await;

        let frame = build_udp_frame(
            [2, 0, 0, 0, 0, 0x0A],
            "10.0.0.1".parse().unwrap(),
            1234,
            "10.0.0.2".parse().unwrap(),
            5678,
        );
        dp.handle_frame(frame).await;

        let out = out_rx_b.recv().await.expect("frame delivered");
        assert_eq!(&out[0..6], &mac_b.0);
    }

    #[tokio::test]
    async fn service_dnat_and_return_snat() {
        let dp = SoftDataplane::new(NodeId(1), 4789);
        let iface_a = InterfaceId(10);
        let iface_b = InterfaceId(11);
        let mac_a = [2, 0, 0, 0, 0, 0x0A];
        let mac_b = [2, 0, 0, 0, 0, 0x0B];

        dp.add_interface(Interface {
            id: iface_a,
            node: NodeId(1),
            kind: InterfaceKind::Veth,
            addresses: vec![],
            mac: Some(MacAddress(mac_a)),
            mtu: None,
        })
        .await
        .unwrap();
        dp.add_interface(Interface {
            id: iface_b,
            node: NodeId(1),
            kind: InterfaceKind::Veth,
            addresses: vec![],
            mac: Some(MacAddress(mac_b)),
            mtu: None,
        })
        .await
        .unwrap();
        dp.add_route(Route {
            destination: "10.0.0.1/32".parse().unwrap(),
            next_hop: NextHop::Local(iface_a),
            weight: 1,
        })
        .await
        .unwrap();
        dp.add_route(Route {
            destination: "10.0.0.2/32".parse().unwrap(),
            next_hop: NextHop::Local(iface_b),
            weight: 1,
        })
        .await
        .unwrap();

        let backend_endpoint = EndpointId(1);
        dp.add_endpoint(Endpoint {
            id: backend_endpoint,
            node: NodeId(1),
            addresses: vec!["10.0.0.2".parse().unwrap()],
            identity: IdentityId(1),
            location: EndpointLocation::Local { interface: iface_b },
        })
        .await
        .unwrap();

        dp.add_service(Service {
            frontend: ServiceFrontend {
                address: "10.0.0.100".parse().unwrap(),
                port: 80,
                protocol: IpProto::Tcp,
            },
            backends: vec![ServiceBackend {
                endpoint: backend_endpoint,
                port: 8080,
                nat: Nat::Destination {
                    address: "10.0.0.2".parse().unwrap(),
                    port: Some(8080),
                },
            }],
        })
        .await
        .unwrap();

        let (io_a, _in_tx_a, mut out_rx_a) = ChannelInterface::new_pair();
        let (io_b, _in_tx_b, mut out_rx_b) = ChannelInterface::new_pair();
        dp.attach_interface(iface_a, io_a).await;
        dp.attach_interface(iface_b, io_b).await;

        // client -> service
        let request = build_tcp_frame(
            mac_a,
            "10.0.0.1".parse().unwrap(),
            40000,
            "10.0.0.100".parse().unwrap(),
            80,
            0x02,
        );
        dp.handle_frame(request).await;
        let delivered = out_rx_b.recv().await.expect("delivered to backend");
        let ip = parse_ipv4(&delivered).unwrap();
        assert_eq!(ip.dst, "10.0.0.2".parse::<Ipv4Addr>().unwrap());

        // backend -> client (reply)
        let reply = build_tcp_frame(
            mac_b,
            "10.0.0.2".parse().unwrap(),
            8080,
            "10.0.0.1".parse().unwrap(),
            40000,
            0x12,
        );
        dp.handle_frame(reply).await;
        let delivered = out_rx_a.recv().await.expect("delivered to client");
        let ip = parse_ipv4(&delivered).unwrap();
        assert_eq!(ip.src, "10.0.0.100".parse::<Ipv4Addr>().unwrap());
    }

    fn eth_ipv4_header(
        src_mac: [u8; 6],
        total_len: u16,
        protocol: u8,
        src_ip: Ipv4Addr,
        dst_ip: Ipv4Addr,
    ) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&[0u8; 6]); // dst mac, filled in by the dataplane on local delivery
        f.extend_from_slice(&src_mac);
        f.extend_from_slice(&0x0800u16.to_be_bytes());
        f.push(0x45); // version 4, ihl 5
        f.push(0);
        f.extend_from_slice(&total_len.to_be_bytes());
        f.extend_from_slice(&0u16.to_be_bytes()); // identification
        f.extend_from_slice(&0u16.to_be_bytes()); // flags/fragment offset
        f.push(64); // ttl
        f.push(protocol);
        f.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        f.extend_from_slice(&src_ip.octets());
        f.extend_from_slice(&dst_ip.octets());
        f
    }

    fn build_udp_frame(
        src_mac: [u8; 6],
        src_ip: Ipv4Addr,
        src_port: u16,
        dst_ip: Ipv4Addr,
        dst_port: u16,
    ) -> Vec<u8> {
        let mut f = eth_ipv4_header(src_mac, 28, 17, src_ip, dst_ip);
        f.extend_from_slice(&src_port.to_be_bytes());
        f.extend_from_slice(&dst_port.to_be_bytes());
        f.extend_from_slice(&8u16.to_be_bytes());
        f.extend_from_slice(&0u16.to_be_bytes());
        f
    }

    fn build_tcp_frame(
        src_mac: [u8; 6],
        src_ip: Ipv4Addr,
        src_port: u16,
        dst_ip: Ipv4Addr,
        dst_port: u16,
        flags: u8,
    ) -> Vec<u8> {
        let mut f = eth_ipv4_header(src_mac, 40, 6, src_ip, dst_ip);
        f.extend_from_slice(&src_port.to_be_bytes());
        f.extend_from_slice(&dst_port.to_be_bytes());
        f.extend_from_slice(&0u32.to_be_bytes()); // seq
        f.extend_from_slice(&0u32.to_be_bytes()); // ack
        f.push(0x50); // data offset 5
        f.push(flags);
        f.extend_from_slice(&0u16.to_be_bytes()); // window
        f.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
        f.extend_from_slice(&0u16.to_be_bytes()); // urgent
        f
    }
}
