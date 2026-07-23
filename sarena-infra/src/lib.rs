use std::{net::Ipv4Addr, path::Path};

use aya::programs::{ProgramError, TcAttachType};
use thiserror::Error;

pub mod bpf;
pub mod mac_address;
pub mod mock_link;
pub mod mock_provisioner;
pub mod netlink_link;
pub mod netlink_provisioner;
pub mod netns;
pub mod tcx;

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
    pub host_mac: Option<MacAddress>,
    pub peer_mac: Option<MacAddress>,
}

/// What `create_veth` hands back.
#[derive(Debug, Clone)]
pub struct VethPair<L> {
    pub host: L,
    pub peer: L,
}

#[allow(async_fn_in_trait)]
pub trait NetworkProvisioner {
    /// Every link this provisioner hands back can also attach/detach TCX
    /// programs.
    type LinkType: Link + TcxAttach;

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

#[allow(async_fn_in_trait)]
pub trait Link {
    fn ifname(&self) -> &str;
    fn ifindex(&self) -> u32;
    fn mac(&self) -> MacAddress;

    /// Bring this link up (set `IFF_UP`).
    async fn set_up(&mut self) -> Res<()>;
    /// Bring this link down (clear `IFF_UP`).
    async fn set_down(&mut self) -> Res<()>;
    /// Set this link's MTU.
    async fn set_mtu(&mut self, mtu: u32) -> Res<()>;
    /// Set this link's hardware (MAC) address.
    async fn set_mac(&mut self, mac: MacAddress) -> Res<()>;
    /// Move this link into a different network namespace.
    async fn set_ns(&mut self, target_ns: &str) -> Res<()>;
    /// Rename this link.
    async fn rename(&mut self, new_name: &str) -> Res<()>;
    /// Delete this link. Deleting either end of a veth pair deletes both.
    async fn delete(&mut self) -> Res<()>;
    async fn set_addr(&mut self, ip: Ipv4Addr, prefix_len: u8) -> Res<()>;
    /// Add (replacing any existing one) the default route (`0.0.0.0/0`) via
    /// `gateway`, routed out through this link.
    async fn add_gateway(&mut self, gateway: Ipv4Addr) -> Res<()>;
}

#[derive(Debug, Clone)]
pub struct PinnedTcxProgram {
    /// The (possibly truncated) program name used to build the pin path.
    pub name: String,
    /// The pinned `bpf_link`'s kernel ID, checked via [`crate::bpf::link_attach_point`].
    pub link_id: u32,
}

#[allow(async_fn_in_trait)]
pub trait TcxAttach {
    type Program;

    /// Attach `prog` at `attach_type` if it isn't already pinned under
    /// `bpffs_dir`, otherwise re-point the existing pin at it.
    ///
    /// `&mut self` even though the real (`NetlinkLink`) implementation
    /// doesn't need to mutate anything: it keeps the receiver consistent
    /// with the rest of this trait/`Link`, so `MockLink` can record calls
    /// into a plain `Vec` field rather than needing interior mutability.
    fn upsert_tcx_program(
        &mut self,
        prog: &mut Self::Program,
        bpffs_dir: impl AsRef<Path>,
        attach_type: TcAttachType,
    ) -> Res<PinnedTcxProgram>;
    /// Remove the pin created by `upsert_tcx_program`, detaching `program`.
    fn detach_tcx(bpffs_dir: impl AsRef<Path>, program: &PinnedTcxProgram) -> Res<()>;
    /// Is `program` currently attached at `attach_type` on this link,
    /// according to the kernel (not just "does a pin file exist")? See the
    /// `&mut self` note on `upsert_tcx_program` -- same reasoning.
    fn has_tcx_link(&mut self, program: &PinnedTcxProgram, attach_type: TcAttachType) -> Res<bool>;
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
