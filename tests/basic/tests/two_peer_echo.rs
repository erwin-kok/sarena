use std::{
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    time::Duration,
};

use aya::maps::{Array, HashMap, Map, MapData};
use ipnet::{IpNet, Ipv4Net};
use sarena_infra::{
    InfraError, InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, Netns,
    NetnsGuard, NetworkProvisioner as _, VethSpec, netlink_link::NetlinkLink, route::Route,
};
use sarena_loader::{AyaBackend, EndpointHandle, EndpointKind, Loader, LoaderHandle, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key, Ipv4KeyExt as _};
use sarena_utils::{LoggingConfig, logging};
use tracing::info;

const PIN_ROOT: &str = "/sys/fs/bpf/sarena";
const IPV4_DEFAULT_ROUTE: IpNet = IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::UNSPECIFIED, 0));

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn two_peer_udp_echo_through_loader() {
    Netns::unshare_self()
        .await
        .expect("failed to unshare a private default namespace");

    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

    info!("Application started");

    let _ = std::fs::remove_dir_all(PIN_ROOT);
    std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let backend = AyaBackend::new(
        format!("{dir}/sarena-ebpf-programs.o"),
        format!("{PIN_ROOT}/globals"),
    );
    let loader = Loader::new(backend, PIN_ROOT);
    let loader_handle = LoaderHandle::spawn(loader, 16);

    let mut provisioner = NetlinkNetworkProvisioner;
    provisioner
        .enable_ip_forwarding(false)
        .await
        .expect("enable_ip_forwarding");

    let gateway_ip = Ipv4Addr::new(10, 0, 0, 5);

    let client1_ns = "client1_ns";
    Netns::create(client1_ns).await.expect("create client1_ns");
    let _guard1 = NetnsGuard::new(client1_ns);

    let peer1_ip = Ipv4Addr::new(192, 168, 1, 10);
    let (mut host1, _) = create_endpoint(
        &mut provisioner,
        &loader_handle,
        "111",
        "host1",
        Netns::path_for(client1_ns),
        gateway_ip,
        peer1_ip,
    )
    .await;

    let client2_ns = "client2_ns";
    Netns::create(client2_ns).await.expect("create client2_ns");
    let _guard2 = NetnsGuard::new(client2_ns);

    let peer2_ip = Ipv4Addr::new(192, 168, 2, 20);
    let (mut host2, _) = create_endpoint(
        &mut provisioner,
        &loader_handle,
        "222",
        "host2",
        Netns::path_for(client2_ns),
        gateway_ip,
        peer2_ip,
    )
    .await;

    let listener =
        Netns::open(client1_ns)
            .expect("open client1_ns")
            .run(move |_handle| async move {
                UdpSocket::bind((peer1_ip, 0)).map_err(InfraError::Runtime)
            })
            .await
            .expect("bind listener");

    listener
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set_read_timeout");
    let listener_addr = listener.local_addr().expect("local_addr");

    let sender =
        Netns::open(client2_ns)
            .expect("open client2_ns")
            .run(move |_handle| async move {
                UdpSocket::bind((peer2_ip, 0)).map_err(InfraError::Runtime)
            })
            .await
            .expect("bind sender");

    let payload = b"hello from peer2";
    sender
        .send_to(payload, listener_addr)
        .expect("send_to failed");

    let mut buf = [0u8; 64];
    let (n, from) = listener.recv_from(&mut buf).expect("recv_from failed");
    assert_eq!(&buf[..n], payload);
    assert_eq!(from.ip(), IpAddr::V4(peer2_ip));

    loader_handle.teardown_all().await.expect("teardown failed");

    host1.delete().await.expect("delete host1");
    host2.delete().await.expect("delete host2");
}

async fn create_endpoint(
    provisioner: &mut NetlinkNetworkProvisioner,
    loader_handle: &LoaderHandle,
    container_id: &str,
    if_name: &str,
    peer_netns_path: PathBuf,
    gateway_ip: Ipv4Addr,
    peer_ip: Ipv4Addr,
) -> (NetlinkLink, NetlinkLink) {
    let lxc_ifname = format!("lxc{container_id}");
    let tmp_peer_ifname = format!("tmp{container_id}");
    let host_mac = MacAddress::generate_rand();
    let lxc_mac = MacAddress::generate_rand();
    let pair = provisioner
        .create_veth(VethSpec {
            host_ifname: lxc_ifname.clone(),
            peer_ifname: tmp_peer_ifname,
            peer_netns: peer_netns_path,
            host_mac: Some(host_mac),
            peer_mac: Some(lxc_mac),
        })
        .await
        .expect("create_veth");
    let (mut host, mut peer) = (pair.host, pair.peer);

    peer.rename(if_name)
        .await
        .expect("could not rename peer interface");

    host.set_rp_filter(0).await.expect("set_rp_filter");

    host.set_mtu(1500).await.expect("set_mtu host");
    peer.set_mtu(1500).await.expect("set_mtu peer");

    host.set_up().await.expect("set_up host");
    peer.set_up().await.expect("set_up peer"); // Should we bring up immediately, or when completely configured?

    peer.set_addr(InterfaceAddress::new(IpAddr::V4(peer_ip), 24).expect("build interface address"))
        .await
        .expect("set_addr peer");

    let routes = ipv4_routes(gateway_ip, 1500);

    for r in routes {
        peer.add_route(&r).await.expect("failed to add route");
    }

    info!(
        "new endpoint -- host_iface = {} (mac: {}), peer_iface = {} (mac: {}, ip: {}, index: {})",
        lxc_ifname,
        host_mac,
        if_name,
        lxc_mac,
        peer_ip,
        peer.ifindex()
    );

    let handle = loader_handle
        .add_endpoint(EndpointKind::Container, &lxc_ifname)
        .await
        .expect("add_endpoint");

    info!("endpoint maps: {:?}", handle.map_paths);

    set_endpoint_config(&handle, host_mac, peer_ip);
    insert_endpoint_info(peer_ip, &host, peer.mac());

    (host, peer)
}

fn set_endpoint_config(handle: &EndpointHandle, host_mac: MacAddress, peer_ip: Ipv4Addr) {
    let path = &handle.map_paths["endpoint_config"];
    let map_data = MapData::from_pin(path).expect("map from_pin");
    let map = Map::Array(map_data);
    let mut array: Array<_, EndpointConfig> = Array::try_from(map).expect("try_from");
    array
        .set(
            0,
            EndpointConfig {
                mac: host_mac.0,
                ipv4: peer_ip,
            },
            0,
        )
        .expect("setting element");
}

fn insert_endpoint_info(peer_ip: Ipv4Addr, link: &NetlinkLink, peer_mac: MacAddress) {
    let pin_root = PinRoot::new(PIN_ROOT);
    let path = pin_root.global_map_dir("lxc_map");
    let map_data = MapData::from_pin(path).expect("map from_pin");
    let map = Map::from_map_data(map_data).expect("from_map_data");

    let mut lxc_map: HashMap<_, Ipv4Key, EndpointInfo> = HashMap::try_from(map).expect("try_from");

    let key = Ipv4Key::from_addr(peer_ip);
    let value = EndpointInfo {
        if_index: link.ifindex(),
        mac: peer_mac.0,
    };
    lxc_map.insert(key, value, 0).expect("insert element");
}

fn ipv4_routes(ip: Ipv4Addr, link_mtu: u32) -> Vec<Route> {
    let ip = IpAddr::V4(ip);

    vec![
        Route {
            prefix: IpNet::from(ip),
            ..Default::default()
        },
        Route {
            prefix: IPV4_DEFAULT_ROUTE,
            nexthop: Some(ip),
            mtu: Some(link_mtu),
            ..Default::default()
        },
    ]
}
