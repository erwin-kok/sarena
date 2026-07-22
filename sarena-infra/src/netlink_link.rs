use std::{
    net::Ipv4Addr,
    os::fd::{AsRawFd, RawFd},
};

use futures::TryStreamExt;
use netlink_packet_route::link::{
    InfoData, InfoKind, InfoVeth, LinkAttribute, LinkFlags, LinkInfo, LinkMessage,
};

use crate::{InfraError, Link, MacAddress, Netns, Res};

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

    /// Network namespace where this Link is located in, if any.
    pub netns: Option<String>,
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
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move { link_set_up_impl(&handle, index).await })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_set_up_impl(&handle, index).await
            }
        }
    }

    async fn set_down(&mut self) -> Res<()> {
        let index = self.index;
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move { link_set_down_impl(&handle, index).await })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_set_down_impl(&handle, index).await
            }
        }
    }

    async fn set_mtu(&mut self, mtu: u32) -> Res<()> {
        let index = self.index;
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move { link_set_mtu_impl(&handle, index, mtu).await })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_set_mtu_impl(&handle, index, mtu).await
            }
        }?;
        self.mtu = Some(mtu);
        Ok(())
    }

    async fn set_mac(&mut self, mac: MacAddress) -> Res<()> {
        let index = self.index;
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move { link_set_mac_impl(&handle, index, mac).await })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_set_mac_impl(&handle, index, mac).await
            }
        }?;
        self.mac = Some(mac);
        Ok(())
    }

    async fn set_ns(&mut self, target_ns: &str) -> Res<()> {
        let target = Netns::open(target_ns)?;
        let target_raw_fd = target.fd.as_raw_fd();
        let index = self.index;
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move {
                        // Keep `target` alive so `target_raw_fd` remains valid.
                        let _keep = target;
                        link_setns_impl(&handle, index, target_raw_fd).await
                    })
                    .await?;
            }
            None => {
                let handle = default_handle().await?;
                // Keep `target` alive so `target_raw_fd` remains valid.
                let _keep = target;
                link_setns_impl(&handle, index, target_raw_fd).await?;
            }
        }
        self.netns = Some(target_ns.to_owned());
        Ok(())
    }

    async fn rename(&mut self, new_name: &str) -> Res<()> {
        let index = self.index;
        let owned_name = new_name.to_owned();
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move {
                        link_rename_impl(&handle, index, &owned_name).await
                    })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_rename_impl(&handle, index, &owned_name).await
            }
        }?;
        self.name = new_name.to_owned();
        Ok(())
    }

    async fn delete(&mut self) -> Res<()> {
        let index = self.index;
        match &self.netns {
            Some(ns) => {
                let netns = Netns::open(ns)?;
                netns
                    .run(move |handle| async move { link_delete_impl(&handle, index).await })
                    .await
            }
            None => {
                let handle = default_handle().await?;
                link_delete_impl(&handle, index).await
            }
        }
    }

    async fn set_addr(&mut self, ip: Ipv4Addr, prefix_len: u8) -> Res<()> {
        // `sarena-infra` doesn't have address (IFA_*) support yet --
        // flagged in the earlier roadmap discussion as a needed-but-not-
        // yet-built `address.rs` sibling to `link.rs`. This is the one
        // operation here that can't be wired up to the current crate.
        let _ = (ip, prefix_len);
        unimplemented!(
            "sarena-infra has no address.rs yet -- see the roadmap discussion for what's needed"
        )
    }
}

pub(crate) async fn create_veth_pair(
    name: &str,
    peer_name: &str,
) -> Result<(NetlinkLink, NetlinkLink), InfraError> {
    let handle = default_handle().await?;

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
    let handle = default_handle().await?;
    let name = name.to_owned();
    get_link_impl(&handle, &name).await
}

/// Return the [`Link`] with the given *name* inside namespace *ns*.
pub(crate) async fn get_link_by_name_in_ns(ns: &str, name: &str) -> Res<NetlinkLink> {
    let netns = Netns::open(ns)?;
    let name = name.to_owned();
    netns
        .run(move |handle| async move { get_link_impl(&handle, &name).await })
        .await
}

/// Return all interfaces visible in the default namespace.
pub(crate) async fn list_links() -> Res<Vec<NetlinkLink>> {
    let handle = default_handle().await?;
    list_links_impl(&handle).await
}

/// Return all interfaces visible inside namespace *ns*.
pub(crate) async fn list_links_in_ns(ns: &str) -> Res<Vec<NetlinkLink>> {
    let netns = Netns::open(ns)?;
    netns
        .run(move |handle| async move { list_links_impl(&handle).await })
        .await
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
    handle
        .link()
        .get()
        .match_name(name.to_owned())
        .execute()
        .try_next()
        .await
        .map_err(InfraError::Netlink)?
        .map(parse_link)
        .ok_or_else(|| InfraError::LinkNotFound(name.to_owned()))
}

/// List all links; called from within an active namespace.
async fn list_links_impl(handle: &rtnetlink::Handle) -> Result<Vec<NetlinkLink>, InfraError> {
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

async fn default_handle() -> Result<rtnetlink::Handle, InfraError> {
    let (conn, handle, _) = rtnetlink::new_connection().map_err(InfraError::Runtime)?;
    tokio::spawn(conn);
    Ok(handle)
}
