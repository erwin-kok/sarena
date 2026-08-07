use std::{
    net::{IpAddr, Ipv4Addr},
    os::fd::{AsRawFd, RawFd},
    path::{Path, PathBuf},
};

use aya::programs::{SchedClassifier, TcAttachType};
use futures::TryStreamExt;
use ipnetwork::IpNetwork;
use netlink_packet_route::{
    address::{AddressAttribute, AddressFlags, AddressHeaderFlags},
    link::{InfoData, InfoKind, InfoVeth, LinkAttribute, LinkFlags, LinkInfo, LinkMessage},
    route::{RouteAttribute, RouteMetric, RouteScope},
};

use crate::{
    InfraError, Link, MacAddress, Netns, PinnedTcxProgram, Res, TcxAttach, route::Route, tcx,
};

/// Recognised `IFLA_INFO_KIND` strings mapped to typed variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    Veth,
    Bridge,
    Dummy,
    Loopback,
    Tun,
    Vlan,
    Macvlan,
    Ipvlan,
    Vxlan,
    Geneve,
    /// Any kind string not explicitly listed above.
    Other(String),
    /// No `IFLA_INFO_KIND` attribute was present (non-loopback physical NICs).
    Unknown,
}

impl From<&InfoKind> for LinkKind {
    fn from(ik: &InfoKind) -> Self {
        match ik {
            InfoKind::Veth => Self::Veth,
            InfoKind::Bridge => Self::Bridge,
            InfoKind::Dummy => Self::Dummy,
            InfoKind::Tun => Self::Tun,
            InfoKind::Vlan => Self::Vlan,
            InfoKind::MacVlan => Self::Macvlan,
            InfoKind::IpVlan => Self::Ipvlan,
            InfoKind::Vxlan => Self::Vxlan,
            InfoKind::Geneve => Self::Geneve,
            // Catch-all: format the debug representation as the "other" string.
            other => Self::Other(format!("{other:?}").to_lowercase()),
        }
    }
}

/// Snapshot of a kernel network interface returned by netlink queries.
///
/// Built from `RTM_NEWLINK` messages; the fields map directly to
/// well-known `IFLA_*` attributes.
#[derive(Debug, Clone)]
pub struct NetlinkLink {
    /// Kernel interface index (`ifindex`).  Stable for the lifetime of the
    /// interface inside the namespace.
    pub index: u32,

    /// Interface name, e.g. `eth0`, `veth0a2f`.  Up to `IFNAMSIZ - 1` bytes.
    pub name: String,

    /// Raw `IFF_*` bitfield from the `ifinfomsg` header.
    /// Common bits: `IFF_UP = 0x1`, `IFF_RUNNING = 0x40`,
    /// `IFF_LOOPBACK = 0x8`, `IFF_BROADCAST = 0x2`.
    pub flags: LinkFlags,

    /// Driver kind derived from `IFLA_INFO_KIND` (veth, bridge, …).
    pub kind: LinkKind,

    /// Hardware (MAC) address from `IFLA_ADDRESS`, if present.
    pub mac: Option<MacAddress>,

    /// MTU from `IFLA_MTU`, if present.
    pub mtu: Option<u32>,

    /// `IFLA_MASTER` – index of the bridge / bond this interface belongs to.
    pub master_index: Option<u32>,

    /// Absolute path to the network namespace where this Link is located,
    /// if any (e.g. `/run/netns/<name>` or `/proc/<pid>/ns/net`).
    pub netns: Option<PathBuf>,
}

impl NetlinkLink {
    /// Returns `true` if the `IFF_UP` flag is set.
    pub const fn is_up(&self) -> bool {
        self.flags.contains(LinkFlags::Up)
    }

    /// Returns `true` if the `IFF_RUNNING` flag is set.
    pub const fn is_running(&self) -> bool {
        self.flags.contains(LinkFlags::Running)
    }
}

impl Link for NetlinkLink {
    fn ifname(&self) -> &str {
        &self.name
    }

    fn ifindex(&self) -> u32 {
        self.index
    }

    fn mac(&self) -> MacAddress {
        // Every net device the kernel creates has a link-layer address;
        // absence here would mean something is wrong at the
        // netlink-parsing level, not a normal case callers should have to
        // handle -- hence `expect`, not another `Result` layer.
        self.mac.expect("kernel always assigns an interface a MAC")
    }

    async fn set_up(&mut self) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_set_up_impl(&handle, index).await })
                .await
        } else {
            let handle = default_handle()?;
            link_set_up_impl(&handle, index).await
        }
    }

    async fn set_down(&mut self) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_set_down_impl(&handle, index).await })
                .await
        } else {
            let handle = default_handle()?;
            link_set_down_impl(&handle, index).await
        }
    }

    async fn set_mtu(&mut self, mtu: u32) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_set_mtu_impl(&handle, index, mtu).await })
                .await?;
        } else {
            let handle = default_handle()?;
            link_set_mtu_impl(&handle, index, mtu).await?;
        }
        self.mtu = Some(mtu);
        Ok(())
    }

    async fn set_mac(&mut self, mac: MacAddress) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_set_mac_impl(&handle, index, mac).await })
                .await?;
        } else {
            let handle = default_handle()?;
            link_set_mac_impl(&handle, index, mac).await?
        }
        self.mac = Some(mac);
        Ok(())
    }

    async fn set_ns(&mut self, target: &Netns) -> Res<()> {
        let target_raw_fd = target.fd.as_raw_fd();
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_setns_impl(&handle, index, target_raw_fd).await })
                .await?;
        } else {
            let handle = default_handle()?;
            link_setns_impl(&handle, index, target_raw_fd).await?;
        }
        self.netns = Some(target.path.clone());
        Ok(())
    }

    async fn rename(&mut self, new_name: &str) -> Res<()> {
        let index = self.index;
        let owned_name = new_name.to_owned();
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                    .run(move |handle| async move {
                        link_rename_impl(&handle, index, &owned_name).await
                    })
                    .await?;
        } else {
            let handle = default_handle()?;
            link_rename_impl(&handle, index, &owned_name).await?
        }
        new_name.clone_into(&mut self.name);
        Ok(())
    }

    async fn delete(&mut self) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_delete_impl(&handle, index).await })
                .await
        } else {
            let handle = default_handle()?;
            link_delete_impl(&handle, index).await
        }
    }

    async fn set_addr(&mut self, addr: IpNetwork) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_add_addr_impl(&handle, index, addr).await })
                .await
        } else {
            let handle = default_handle()?;
            link_add_addr_impl(&handle, index, addr).await
        }
    }

    async fn add_gateway(&mut self, gateway: Ipv4Addr) -> Res<()> {
        let index = self.index;
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                    .run(move |handle| async move {
                        link_add_gateway_impl(&handle, index, gateway).await
                    })
                    .await
        } else {
            let handle = default_handle()?;
            link_add_gateway_impl(&handle, index, gateway).await
        }
    }

    async fn add_route(&mut self, route: &Route) -> Res<()> {
        let index = self.index;
        let route = route.clone();
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |handle| async move { link_add_route_impl(&handle, index, &route).await })
                .await
        } else {
            let handle = default_handle()?;
            link_add_route_impl(&handle, index, &route).await
        }
    }

    async fn set_ipv4_forwarding(&mut self, enabled: bool) -> Res<()> {
        let path = format!("/proc/sys/net/ipv4/conf/{}/forwarding", self.name);
        let value = if enabled { "1" } else { "0" };
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |_| async move { sysctl_write(&path, value) })
                .await
        } else {
            sysctl_write(&path, value)
        }
    }

    async fn set_ipv6_forwarding(&mut self, enabled: bool) -> Res<()> {
        let path = format!("/proc/sys/net/ipv6/conf/{}/forwarding", self.name);
        let value = if enabled { "1" } else { "0" };
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |_| async move { sysctl_write(&path, value) })
                .await
        } else {
            sysctl_write(&path, value)
        }
    }

    async fn set_ipv6_disable(&mut self, disable: bool) -> Res<()> {
        let path = format!("/proc/sys/net/ipv6/conf/{}/disable_ipv6", self.name);
        let value = if disable { "1" } else { "0" };
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |_| async move { sysctl_write(&path, value) })
                .await
        } else {
            sysctl_write(&path, value)
        }
    }

    async fn set_rp_filter(&mut self, value: u8) -> Res<()> {
        let path = format!("/proc/sys/net/ipv4/conf/{}/rp_filter", self.name);
        let val = value.to_string();
        if let Some(ns) = &self.netns {
            let netns = Netns::open_path(ns)?;
            netns
                .run(move |_| async move { sysctl_write(&path, &val) })
                .await
        } else {
            sysctl_write(&path, &val)
        }
    }
}

impl TcxAttach for NetlinkLink {
    type Program = SchedClassifier;

    fn upsert_tcx_program(
        &mut self,
        prog: &mut SchedClassifier,
        bpffs_dir: impl AsRef<Path>,
        attach_type: TcAttachType,
    ) -> Res<PinnedTcxProgram> {
        // See `TcxAttach::upsert_tcx_program`'s doc comment: only a link in
        // the caller's own namespace can have a program attached, since a
        // pin created from here would land in the wrong place otherwise.
        if let Some(netns) = &self.netns {
            return Err(InfraError::TcxRequiresLocalLink {
                ifname: self.name.clone(),
                netns: netns.clone(),
            });
        }
        tcx::upsert_tcx(self, prog, bpffs_dir, attach_type)
    }

    fn has_tcx_link(&mut self, program: &str, attach_type: TcAttachType) -> Res<bool> {
        tcx::has_tcx(self, program, attach_type)
    }
}

pub(crate) async fn create_veth_pair(
    name: &str,
    peer_name: &str,
) -> Res<(NetlinkLink, NetlinkLink)> {
    let handle = default_handle()?;

    let mut peer_msg = LinkMessage::default();
    peer_msg
        .attributes
        .push(LinkAttribute::IfName(peer_name.to_owned()));

    let mut msg = LinkMessage::default();
    msg.attributes.push(LinkAttribute::IfName(name.to_owned()));
    msg.attributes.push(LinkAttribute::LinkInfo(vec![
        LinkInfo::Kind(InfoKind::Veth),
        LinkInfo::Data(InfoData::Veth(InfoVeth::Peer(peer_msg))),
    ]));

    handle
        .link()
        .add(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)?;

    let link = get_link_impl(&handle, name).await?;
    let peer = get_link_impl(&handle, peer_name).await?;
    Ok((link, peer))
}

/// Return the [`Link`] with the given *name*.
pub(crate) async fn get_link_by_name(name: &str) -> Res<NetlinkLink> {
    let handle = default_handle()?;
    let name = name.to_owned();
    get_link_impl(&handle, &name).await
}

/// Return the [`Link`] with the given *name* inside namespace `ns`.
pub(crate) async fn get_link_by_name_in_ns(ns: &Netns, name: &str) -> Res<NetlinkLink> {
    let netns = Netns::open_path(&ns.path)?;
    let name = name.to_owned();
    let mut link = netns
        .run(move |handle| async move { get_link_impl(&handle, &name).await })
        .await?;
    // `parse_link` (called from inside the entered namespace above) has no
    // way to know its own name -- stamp it here so the returned snapshot
    // actually reflects where it was fetched from, instead of always
    // looking like a default-namespace link.
    link.netns = Some(ns.path.clone());
    Ok(link)
}

/// Return all interfaces visible in the default namespace.
pub(crate) async fn list_links() -> Res<Vec<NetlinkLink>> {
    let handle = default_handle()?;
    list_links_impl(&handle).await
}

/// Return all interfaces visible inside namespace `ns`.
pub(crate) async fn list_links_in_ns(ns: &Netns) -> Res<Vec<NetlinkLink>> {
    let netns = Netns::open_path(&ns.path)?;
    let mut links = netns
        .run(move |handle| async move { list_links_impl(&handle).await })
        .await?;
    // See `get_link_by_name_in_ns` -- same reasoning.
    for link in &mut links {
        link.netns = Some(ns.path.clone());
    }
    Ok(links)
}

/// Convert a raw `RTM_NEWLINK` message into a [`Link`].
fn parse_link(msg: LinkMessage) -> NetlinkLink {
    let index = msg.header.index;

    let flags = msg.header.flags;

    let mut name = String::new();
    let mut kind = LinkKind::Unknown;
    let mut mac: Option<MacAddress> = None;
    let mut mtu: Option<u32> = None;
    let mut master_index: Option<u32> = None;

    for attr in &msg.attributes {
        match attr {
            LinkAttribute::IfName(n) => name.clone_from(n),
            LinkAttribute::Mtu(m) => mtu = Some(*m),
            LinkAttribute::Controller(idx) => master_index = Some(*idx),
            LinkAttribute::Address(bytes) if bytes.len() == 6 => {
                let mut arr = [0u8; 6];
                arr.copy_from_slice(bytes);
                mac = Some(MacAddress(arr));
            }
            LinkAttribute::LinkInfo(infos) => {
                for info in infos {
                    if let LinkInfo::Kind(ik) = info {
                        kind = LinkKind::from(ik);
                    }
                }
            }
            _ => {}
        }
    }

    // Loopback has no IFLA_INFO_KIND; detect via IFF_LOOPBACK (bit 3).
    if kind == LinkKind::Unknown && flags.contains(LinkFlags::Loopback) {
        kind = LinkKind::Loopback;
    }

    NetlinkLink {
        index,
        name,
        flags,
        kind,
        mac,
        mtu,
        master_index,
        netns: None,
    }
}

/// Fetch a single link by name; called from within an active namespace.
async fn get_link_impl(handle: &rtnetlink::Handle, name: &str) -> Res<NetlinkLink> {
    // `match_name` makes this a targeted (non-dump) RTM_GETLINK: a name the
    // kernel doesn't recognize comes back as an ENODEV netlink error, not
    // an empty stream. The `ok_or_else` below is retained as a fallback in
    // case some kernel version instead returns a successful empty result.
    let msg = match handle
        .link()
        .get()
        .match_name(name.to_owned())
        .execute()
        .try_next()
        .await
    {
        Ok(msg) => msg,
        Err(rtnetlink::Error::NetlinkError(err))
            if err
                .code
                .is_some_and(|code| code.get() == -(nix::errno::Errno::ENODEV as i32)) =>
        {
            return Err(InfraError::LinkNotFound(name.to_owned()));
        }
        Err(err) => return Err(InfraError::Netlink(err)),
    };

    msg.map(parse_link)
        .ok_or_else(|| InfraError::LinkNotFound(name.to_owned()))
}

/// List all links; called from within an active namespace.
async fn list_links_impl(handle: &rtnetlink::Handle) -> Res<Vec<NetlinkLink>> {
    handle
        .link()
        .get()
        .execute()
        .map_ok(parse_link)
        .try_collect::<Vec<_>>()
        .await
        .map_err(InfraError::Netlink)
}

/// Bring the link with the given index up; called from within an active namespace.
async fn link_set_up_impl(handle: &rtnetlink::Handle, index: u32) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.header.flags = LinkFlags::Up;
    msg.header.change_mask = LinkFlags::Up;
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Bring the link with the given index down; called from within an active namespace.
async fn link_set_down_impl(handle: &rtnetlink::Handle, index: u32) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.header.flags = LinkFlags::empty();
    msg.header.change_mask = LinkFlags::Up;
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Set the MTU of the link with the given index; called from within an active namespace.
async fn link_set_mtu_impl(handle: &rtnetlink::Handle, index: u32, mtu: u32) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.attributes.push(LinkAttribute::Mtu(mtu));
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Set the MAC address of the link with the given index; called from within an active namespace.
async fn link_set_mac_impl(handle: &rtnetlink::Handle, index: u32, mac: MacAddress) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.attributes.push(LinkAttribute::Address(mac.0.to_vec()));
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Set (replacing any existing matching entry) an address on the link with
/// the given index; called from within an active namespace. IPv6 addresses
/// are added with `IFA_F_NODAD` -- skipping duplicate address detection,
/// which would otherwise leave the address `tentative` (and generally
/// unusable) for a few seconds after being added.
async fn link_add_addr_impl(handle: &rtnetlink::Handle, index: u32, addr: IpNetwork) -> Res<()> {
    let mut request = handle
        .address()
        .add(index, addr.ip(), addr.prefix())
        .replace();
    if addr.is_ipv6() {
        let message = request.message_mut();
        message.header.flags |= AddressHeaderFlags::Nodad;
        message
            .attributes
            .push(AddressAttribute::Flags(AddressFlags::Nodad));
    }
    request.execute().await.map_err(InfraError::Netlink)
}

/// Add (replacing any existing one) the default route via `gateway`,
/// routed out through the link with the given index; called from within an
/// active namespace.
async fn link_add_gateway_impl(
    handle: &rtnetlink::Handle,
    index: u32,
    gateway: Ipv4Addr,
) -> Res<()> {
    let route = rtnetlink::RouteMessageBuilder::<Ipv4Addr>::new()
        .gateway(gateway)
        .output_interface(index)
        .build();
    handle
        .route()
        .add(route)
        .replace()
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Adds `route` via the link with the given index; called from within an
/// active namespace.
///
/// Scope defaults to universe, but drops to link-local when there's no
/// nexthop (an on-link/directly-connected route) -- this is computed from
/// `route.nexthop`, overriding whatever `route.scope` already holds,
/// exactly like the reference implementation this is ported from (which
/// likewise ignores its own route's `Scope` field here in favor of
/// deriving it from whether a nexthop is present).
async fn link_add_route_impl(handle: &rtnetlink::Handle, index: u32, route: &Route) -> Res<()> {
    let mut builder: rtnetlink::RouteMessageBuilder =
        rtnetlink::RouteMessageBuilder::<IpAddr>::new()
            .destination_prefix(route.prefix.ip(), route.prefix.prefix())
            .map_err(|e| InfraError::InvalidRoute(e.to_string()))?
            .output_interface(index);

    builder = match route.nexthop {
        Some(nexthop) => builder
            .gateway(nexthop)
            .map_err(|e| InfraError::InvalidRoute(e.to_string()))?,
        None => builder.scope(RouteScope::Link),
    };

    if let Some(table) = route.table {
        builder = builder.table_id(table);
    }

    let mut request = handle.route().add(builder.build()).replace();
    if let Some(mtu) = route.mtu {
        request
            .message_mut()
            .attributes
            .push(RouteAttribute::Metrics(vec![RouteMetric::Mtu(mtu)]));
    }

    request.execute().await.map_err(InfraError::Netlink)
}

/// Rename the link with the given index; called from within an active namespace.
async fn link_rename_impl(handle: &rtnetlink::Handle, index: u32, new_name: &str) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.attributes
        .push(LinkAttribute::IfName(new_name.to_owned()));
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Delete the link with the given index; called from within an active namespace.
async fn link_delete_impl(handle: &rtnetlink::Handle, index: u32) -> Res<()> {
    handle
        .link()
        .del(index)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

/// Move the link with the given index into the namespace referenced by
/// `target_raw_fd`; called from within an active namespace.
async fn link_setns_impl(handle: &rtnetlink::Handle, index: u32, target_raw_fd: RawFd) -> Res<()> {
    let mut msg = LinkMessage::default();
    msg.header.index = index;
    msg.attributes.push(LinkAttribute::NetNsFd(target_raw_fd));
    handle
        .link()
        .set(msg)
        .execute()
        .await
        .map_err(InfraError::Netlink)
}

fn default_handle() -> Res<rtnetlink::Handle> {
    let (conn, handle, _) = rtnetlink::new_connection().map_err(InfraError::Runtime)?;
    tokio::spawn(conn);
    Ok(handle)
}

pub(crate) fn sysctl_write(path: &str, value: &str) -> Res<()> {
    std::fs::write(path, value).map_err(|e| InfraError::Io {
        context: format!("write {path}"),
        source: e,
    })
}
