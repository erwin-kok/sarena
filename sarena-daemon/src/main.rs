use std::{
    fs,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    time::Duration,
};

use aya::maps::{Array, HashMap, Map, MapData};
use sarena_daemon::{add::endpoint_to_ifname, ipam::ipv4_routes, types::CmdArgs};
use sarena_infra::{
    InfraError, InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, Netns,
    NetnsGuard, NetworkProvisioner as _, VethSpec, netlink_link::NetlinkLink,
};
use sarena_loader::{AyaBackend, EndpointHandle, EndpointKind, Loader, LoaderHandle, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key, Ipv4KeyExt as _};
use sarena_utils::{LoggingConfig, logging};
use tracing::info;

const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    // Must run before anything else on this thread: makes "the default
    // namespace" for the rest of this process a fresh, private one instead
    // of the machine's real default -- see `Netns::unshare_self`'s doc
    // comment for why this requires `current_thread` (no worker pool to
    // migrate onto later). Every host-side veth end created below lands
    // here, since `HostLink` never moves once created.
    Netns::unshare_self().await?;

    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

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

    let mut provisioner = NetlinkNetworkProvisioner;
    provisioner.enable_ip_forwarding(false).await.expect("");

    let gateway_ip = Ipv4Addr::new(10, 0, 0, 5);

    let client1_ns = "client1_ns";
    Netns::create(client1_ns).await?;
    let _guard1 = NetnsGuard::new(client1_ns);

    let host1_name = "host1";
    let args1 = CmdArgs {
        container_id: "111".to_string(),
        netns: Netns::path_for(client1_ns),
        if_name: host1_name.to_string(),
        args: None,
        path: String::new(),
        stdin_data: vec![],
        netns_override: None,
    };

    let peer1_ip = Ipv4Addr::new(192, 168, 1, 10);

    let (mut host1, _) = create_endpoint(
        &mut provisioner,
        &loader_handle,
        &args1,
        gateway_ip,
        peer1_ip,
    )
    .await?;

    let client2_ns = "client2_ns";
    Netns::create(client2_ns).await?;
    let _guard2 = NetnsGuard::new(client2_ns);

    let host2_name = "host2";
    let args2 = CmdArgs {
        container_id: "222".to_string(),
        netns: Netns::path_for(client2_ns),
        if_name: host2_name.to_string(),
        args: None,
        path: String::new(),
        stdin_data: vec![],
        netns_override: None,
    };

    let peer2_ip = Ipv4Addr::new(192, 168, 2, 20);
    let (mut host2, _) = create_endpoint(
        &mut provisioner,
        &loader_handle,
        &args2,
        gateway_ip,
        peer2_ip,
    )
    .await?;

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

    loader_handle.teardown_all().await.expect("teardown failed");

    host1.delete().await?;
    host2.delete().await?;

    Ok(())
}

async fn create_endpoint(
    provisioner: &mut NetlinkNetworkProvisioner,
    loader_handle: &LoaderHandle,
    args: &CmdArgs,
    gateway_ip: Ipv4Addr,
    peer_ip: Ipv4Addr,
) -> Result<(NetlinkLink, NetlinkLink), anyhow::Error> {
    let lxc_ifname = endpoint_to_ifname(&format!("{}:{}", args.container_id, args.if_name));
    let host_mac = MacAddress::generate_rand();
    let lxc_mac = MacAddress::generate_rand();
    let pair = provisioner
        .create_veth(VethSpec {
            host_ifname: lxc_ifname.clone(),
            peer_ifname: args.if_name.clone(),
            peer_netns: args.netns.clone(),
            host_mac: Some(host_mac),
            peer_mac: Some(lxc_mac),
        })
        .await?;
    let (mut host, mut peer) = (pair.host, pair.peer);

    host.set_rp_filter(0).await?;

    host.set_mtu(1500).await?;
    peer.set_mtu(1500).await?;

    host.set_up().await?;
    peer.set_up().await?; // Should we bring up immediately, or when completely configured?

    peer.set_addr(InterfaceAddress::new(IpAddr::V4(peer_ip), 24)?)
        .await?;

    let routes = ipv4_routes(gateway_ip, 1500);

    for r in routes {
        peer.add_route(&r).await.expect("failed to add route");
    }

    info!(
        "new endpoint -- host_iface = {} (mac: {}), peer_iface = {} (mac: {}, ip: {}, index: {})",
        lxc_ifname,
        host_mac,
        args.if_name,
        lxc_mac,
        peer_ip,
        peer.ifindex()
    );

    let handle = loader_handle
        .add_endpoint(EndpointKind::Container, &lxc_ifname)
        .await?;

    info!("endpoint maps: {:?}", handle.map_paths);

    set_endpoint_config(&handle, host_mac, peer_ip)?;

    insert_endpoint_info(peer_ip, &host, peer.mac())?;

    Ok((host, peer))
}

fn set_endpoint_config(
    handle: &EndpointHandle,
    host_mac: MacAddress,
    peer_ip: Ipv4Addr,
) -> Result<(), anyhow::Error> {
    let path = &handle.map_paths["endpoint_config"];
    let map_data = MapData::from_pin(path)?;
    let map = Map::Array(map_data);
    let mut array: Array<_, EndpointConfig> = Array::try_from(map)?;
    array.set(
        0,
        EndpointConfig {
            mac: host_mac.0,
            ipv4: peer_ip,
        },
        0,
    )?;

    Ok(())
}

fn insert_endpoint_info(
    peer_ip: Ipv4Addr,
    link: &NetlinkLink,
    peer_mac: MacAddress,
) -> Result<(), anyhow::Error> {
    let pin_root = PinRoot::new(PIN_ROOT);
    let path = pin_root.global_map_dir("lxc_map");
    let map_data = MapData::from_pin(path)?;
    let map = Map::from_map_data(map_data)?;

    let mut lxc_map: HashMap<_, Ipv4Key, EndpointInfo> = HashMap::try_from(map)?;
    let key = Ipv4Key::from_addr(peer_ip);
    let value = EndpointInfo {
        if_index: link.ifindex(),
        mac: peer_mac.0,
    };
    lxc_map.insert(key, value, 0)?;

    Ok(())
}
