use std::{
    fs,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    time::Duration,
};

use aya::{
    EbpfError,
    maps::MapError,
    programs::{ProgramError, TcAttachType},
};
use ipnetwork::IpNetwork;
use sarena_infra::{
    InfraError, Link as _, NetlinkNetworkProvisioner, Netns, NetnsGuard, NetworkProvisioner as _,
    TcxAttach as _, VethSpec,
};
use sarena_loader::{AyaBackend, EndpointKind, Loader, LoaderHandle};
use tokio::signal;
use tracing::info;

const PIN_ROOT: &str = "/sys/fs/bpf/test";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    // Must run before anything else on this thread: makes "the default
    // namespace" for the rest of this process a fresh, private one instead
    // of the machine's real default -- see `Netns::unshare_self`'s doc
    // comment for why this requires `current_thread` (no worker pool to
    // migrate onto later). Every host-side veth end created below lands
    // here, since `HostLink` never moves once created.
    Netns::unshare_self().await?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing_log::LogTracer::init()
        .expect("failed to install LogTracer bridging `log` records into `tracing`");

    info!("Application started");

    let _ = fs::remove_dir_all(PIN_ROOT);

    std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let backend = AyaBackend::new(
        format!("{dir}/sarena-ebpf-programs.o"),
        format!("{PIN_ROOT}/globals"),
    );
    let loader = Loader::new(backend, PIN_ROOT);
    let loader_handle = LoaderHandle::spawn(loader, 16);

    let client1_ns = "client1_ns";
    Netns::create(client1_ns).await?;
    let _guard1 = NetnsGuard::new(client1_ns);

    let client2_ns = "client2_ns";
    Netns::create(client2_ns).await?;
    let _guard2 = NetnsGuard::new(client2_ns);

    let host1_name = "host1";
    let peer1_name = "peer1";

    let host2_name = "host2";
    let peer2_name = "peer2";

    let mut provisioner = NetlinkNetworkProvisioner;

    // Pair 1: host1 <-> peer1. `create_veth` moves the peer end into
    // `client1_ns` as part of creation; the host end stays in this
    // process's own namespace (`HostLink` never moves).
    let pair1 = provisioner
        .create_veth(VethSpec {
            host_ifname: host1_name.to_string(),
            peer_ifname: peer1_name.to_string(),
            peer_netns: client1_ns.to_string(),
            host_mac: None,
            peer_mac: None,
        })
        .await?;
    let (mut host1, mut peer1) = (pair1.host, pair1.peer);

    // Pair 2: host2 <-> peer2.
    let pair2 = provisioner
        .create_veth(VethSpec {
            host_ifname: host2_name.to_string(),
            peer_ifname: peer2_name.to_string(),
            peer_netns: client2_ns.to_string(),
            host_mac: None,
            peer_mac: None,
        })
        .await?;
    let (mut host2, mut peer2) = (pair2.host, pair2.peer);

    let host1_ip = Ipv4Addr::new(192, 168, 21, 1);
    let peer1_ip = Ipv4Addr::new(192, 168, 21, 21);
    let host2_ip = Ipv4Addr::new(192, 168, 22, 1);
    let peer2_ip = Ipv4Addr::new(192, 168, 22, 22);

    host1.set_up().await?;
    host1
        .set_addr(IpNetwork::new(IpAddr::V4(host1_ip), 24)?)
        .await?;
    host2.set_up().await?;
    host2
        .set_addr(IpNetwork::new(IpAddr::V4(host2_ip), 24)?)
        .await?;

    peer1.set_up().await?;
    peer1
        .set_addr(IpNetwork::new(IpAddr::V4(peer1_ip), 24)?)
        .await?;
    peer1.add_gateway(host1_ip).await?;

    peer2.set_up().await?;
    peer2
        .set_addr(IpNetwork::new(IpAddr::V4(peer2_ip), 24)?)
        .await?;
    peer2.add_gateway(host2_ip).await?;

    // Host ends live in this process's own namespace, so this runs
    // directly -- no namespace switch needed.
    std::fs::write("/proc/sys/net/ipv4/ip_forward", b"1")?;
    std::fs::write(
        format!("/proc/sys/net/ipv4/conf/{host1_name}/forwarding"),
        b"1",
    )?;
    std::fs::write(
        format!("/proc/sys/net/ipv4/conf/{host2_name}/forwarding"),
        b"1",
    )?;

    info!("host1 index: {}", host1.ifindex());
    info!("host2 index: {}", host2.ifindex());

    loader_handle
        .add_endpoint(EndpointKind::Host, host1_name)
        .await
        .expect("add_endpoint (host1) failed");
    loader_handle
        .add_endpoint(EndpointKind::Host, host2_name)
        .await
        .expect("add_endpoint (host2) failed");

    assert!(
        host1
            .has_tcx_link("from_host", TcAttachType::Ingress)
            .expect("expect program")
    );

    let listener =
        Netns::open(client1_ns)?
            .run(move |_handle| async move {
                UdpSocket::bind((peer1_ip, 0)).map_err(InfraError::Runtime)
            })
            .await?;

    listener.set_read_timeout(Some(Duration::from_secs(5)))?;
    let listener_addr = listener.local_addr()?;

    let sender =
        Netns::open(client2_ns)?
            .run(move |_handle| async move {
                UdpSocket::bind((peer2_ip, 0)).map_err(InfraError::Runtime)
            })
            .await?;

    let payload = b"hello from peer2";
    sender
        .send_to(payload, listener_addr)
        .expect("send_to failed");

    let mut buf = [0u8; 64];
    let (n, from) = listener.recv_from(&mut buf)?;
    assert_eq!(&buf[..n], payload);
    assert_eq!(from.ip(), IpAddr::V4(peer2_ip));

    let ctrl_c = signal::ctrl_c();
    info!("Waiting for Ctrl-C...");
    ctrl_c.await?;
    info!("Exiting...");

    loader_handle.teardown_all().await.expect("teardown failed");

    host1.delete().await?;
    host2.delete().await?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("eBPF program error: {0}")]
    EbpfError(#[from] EbpfError),

    #[error("eBPF program error: {0}")]
    ProgramError(#[from] ProgramError),

    #[error("Program {0} not found")]
    ProgramNotFound(String),

    #[error("eBPF map error: {0}")]
    MapError(#[from] MapError),

    #[error("Map {0} not found")]
    MapNotFound(String),
}

pub type Res<T> = Result<T, DaemonError>;
