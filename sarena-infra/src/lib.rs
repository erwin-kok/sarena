use std::net::Ipv4Addr;

use aya::programs::ProgramError;
use thiserror::Error;

pub mod mac_address;
pub mod mock_link;
pub mod mock_provisioner;
pub mod netlink_link;
pub mod netlink_provisioner;
pub mod netns;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use mac_address::MacAddress;
pub use mock_provisioner::MockNetworkProvisioner;
pub use netlink_provisioner::NetlinkNetworkProvisioner;
pub use netns::Netns;

/// Everything needed to create one veth pair: the host side stays in the
/// router's own netns, the peer side gets moved into `peer_netns`.
#[derive(Debug, Clone)]
pub struct VethSpec {
    pub host_ifname: String,
    pub peer_ifname: String,
    pub peer_netns: String,
    /// Pin a specific MAC rather than letting the kernel generate a random
    /// one -- worth doing on the host side particularly, since that MAC
    /// becomes the port's ARP identity. Deterministic MACs also make logs
    /// and tests reproducible.
    pub host_mac: Option<MacAddress>,
    pub peer_mac: Option<MacAddress>,
}

/// What `create_veth` hands back. Generic over the link type so both the
/// real (`Link`) and mock (`MockLink`) implementations can
/// produce one without either side needing to know about the other.
#[derive(Debug, Clone)]
pub struct VethPair<L> {
    pub host: L,
    pub peer: L,
}

/// High-level lifecycle: bringing namespaces and veth pairs into existence,
/// tearing them down, and querying what currently exists. Deliberately
/// *doesn't* include per-link mutation (`set_up`, `set_addr`, ...) -- see
/// [`LinkHandle`].
#[allow(async_fn_in_trait)]
pub trait NetworkProvisioner {
    type LinkType: Link;

    async fn create_netns(&mut self, name: &str) -> Res<()>;
    async fn delete_netns(&mut self, netns: &str) -> Res<()>;
    async fn create_veth(&mut self, spec: VethSpec) -> Res<VethPair<Self::LinkType>>;
    /// Deleting either end of a veth pair deletes both; takes the host end
    /// since that's the one guaranteed to still be reachable from the
    /// router's own namespace.
    async fn delete_veth(&mut self, veth_pair: &mut VethPair<Self::LinkType>) -> Res<()>;
    /// Look up a link by name in the default namespace.
    async fn get_link(&self, name: &str) -> Res<Self::LinkType>;
    /// Look up a link by name inside namespace *ns*.
    async fn get_link_in_ns(&self, ns: &str, name: &str) -> Res<Self::LinkType>;
    /// List all links visible in the default namespace.
    async fn list_links(&self) -> Res<Vec<Self::LinkType>>;
    /// List all links visible inside namespace *ns*.
    async fn list_links_in_ns(&self, ns: &str) -> Res<Vec<Self::LinkType>>;
}

/// Per-link operations -- things you do to an interface that already
/// exists. Kept separate from [`NetworkProvisioner`] so these read as
/// properties of the link itself (`link.set_up().await`) while staying
/// independently mockable.
#[allow(async_fn_in_trait)]
pub trait Link {
    fn ifname(&self) -> &str;
    fn ifindex(&self) -> u32;
    fn mac(&self) -> MacAddress;

    async fn set_up(&mut self) -> Res<()>;
    async fn set_down(&mut self) -> Res<()>;
    async fn set_mtu(&mut self, mtu: u32) -> Res<()>;
    async fn set_mac(&mut self, mac: MacAddress) -> Res<()>;
    /// Move this link into a different network namespace.
    async fn set_ns(&mut self, target_ns: &str) -> Res<()>;
    /// Rename this link.
    async fn rename(&mut self, new_name: &str) -> Res<()>;
    /// Delete this link. Deleting either end of a veth pair deletes both.
    async fn delete(&mut self) -> Res<()>;
    /// Not used for the peer side in practice -- what happens inside
    /// `peer_netns` is a different component's job (a CNI-equivalent),
    /// same scoping call as the rest of this project -- but kept on the
    /// trait rather than split onto an asymmetric host-only type, so
    /// `VethPair<L>` can stay a single, uniform `L` for both ends.
    async fn set_addr(&mut self, ip: Ipv4Addr, prefix_len: u8) -> Res<()>;
}

#[derive(Debug, Error)]
pub enum InfraError {
    #[error("MAC address must be 6 bytes, got {0}")]
    InvalidMac(usize),

    #[error("failed to open network namespace {name:?}")]
    OpenNamespace {
        name: String,
        #[source]
        source: std::io::Error,
    },

    #[error("setns into {path:?} failed")]
    SetNs {
        path: String,
        #[source]
        source: nix::errno::Errno,
    },

    /// The namespaced operation itself completed (successfully or not), but
    /// restoring the OS thread's original namespace afterwards failed. This
    /// is surfaced rather than swallowed: see [`crate::netns::Netns::run`]
    /// docs for why.
    #[error("failed to restore original namespace via {path:?} after operation completed")]
    RestoreNamespace {
        path: String,
        #[source]
        source: nix::errno::Errno,
    },

    #[error("io error: {0}")]
    Runtime(#[source] std::io::Error),

    #[error("dedicated namespace-operation thread panicked before returning a result")]
    ThreadPanicked,

    #[error("failed to spawn dedicated namespace-operation thread")]
    SpawnThread(#[source] std::io::Error),

    #[error("netlink operation failed")]
    Netlink(#[source] rtnetlink::Error),

    #[error("link {0:?} not found")]
    LinkNotFound(String),

    #[error("namespace {0:?} already exists")]
    NamespaceExists(String),

    #[error("namespace {0:?} does not exist")]
    NamespaceNotFound(String),

    #[error("failed to create namespace (unshare)")]
    CreateNamespace(#[source] nix::errno::Errno),

    #[error("mount failed for {target:?}")]
    Mount {
        target: String,
        #[source]
        source: nix::errno::Errno,
    },

    #[error("unmount failed for {target:?}")]
    Unmount {
        target: String,
        #[source]
        source: nix::errno::Errno,
    },

    #[error("flock on netns lock file failed")]
    Flock(#[source] nix::errno::Errno),

    #[error("I/O error during {context}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("BPF query failed during {context} (errno {errno})")]
    BpfQuery { context: String, errno: i32 },

    #[error("eBPF program error: {0}")]
    ProgramError(#[from] ProgramError),

    #[error("link error: {0}")]
    LinkError(#[from] aya::programs::links::LinkError),

    #[error("pin error: {0}")]
    PinError(#[from] aya::pin::PinError),
}

pub type Res<T> = Result<T, InfraError>;
