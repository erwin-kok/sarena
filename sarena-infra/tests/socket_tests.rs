use std::{
    net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket},
    time::Duration,
};

use ipnetwork::IpNetwork;
use sarena_infra::{
    InfraError, Link, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, VethSpec, test_support,
};

/// Shorthand for building the `IpNetwork` [`Link::set_addr`] now takes,
/// from the plain `Ipv4Addr` + prefix length this file otherwise deals in.
fn v4(ip: Ipv4Addr, prefix: u8) -> IpNetwork {
    IpNetwork::new(IpAddr::V4(ip), prefix).expect("valid IPv4 network")
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn open_socket_on_configured_veth_peer() {
    test_support::with_temp_netns("dpi-sock-", |peer_ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let host_name = test_support::unique_name("dpisk0-");
        let peer_name = test_support::unique_name("dpisk1-");

        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: host_name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: peer_ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let (mut host, mut peer) = (pair.host, pair.peer);

        let ip = Ipv4Addr::new(192, 168, 20, 20);
        let gateway = Ipv4Addr::new(192, 168, 20, 1);

        peer.set_up().await.expect("set_up failed");
        peer.set_addr(v4(ip, 24)).await.expect("set_addr failed");
        peer.add_gateway(gateway).await.expect("add_gateway failed");

        let bound_addr = Netns::open_path(&peer_ns)
            .unwrap()
            .run(move |_handle| async move {
                let listener = TcpListener::bind((ip, 0)).map_err(InfraError::Runtime)?;
                Ok(listener.local_addr().unwrap())
            })
            .await
            .expect("failed to open a socket on the peer veth's address");

        assert_eq!(bound_addr.ip(), ip);

        host.delete().await.expect("cleanup delete failed");
    })
    .await;
}

/// Two peer namespaces, each connected via its own veth pair to host ends
/// that stay in the *default* namespace (`HostLink` never moves), with IP
/// forwarding enabled there so a UDP datagram sent from one peer is
/// actually routed to the other.
///
/// `Netns::unshare_self` (first line below, before anything else runs on
/// this `current_thread` test) makes "the default namespace" for this test
/// a fresh, private one instead of the machine's real default -- otherwise
/// the host ends would land in the real default namespace, which on a dev
/// box running Docker/Kubernetes typically has a `FORWARD`-chain policy
/// that only accepts traffic for its own managed interfaces and silently
/// drops everything else, which is exactly what broke this test before
/// this was added.
///
/// Each link gets its *own* subnet (`192.168.21.0/24`, `192.168.22.0/24`)
/// rather than sharing a single `192.168.20.0/24` -- the router-side end of
/// each link has to own that link's gateway address, and two different
/// interfaces can't both own the same IP in the same namespace, so a
/// shared subnet across two separate point-to-point links isn't routable
/// without extra machinery (bridging, or onlink routes + proxy ARP).
#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn forward_udp_packet_between_two_peer_namespaces() {
    Netns::unshare_self()
        .await
        .expect("unshare_self failed -- needs CAP_NET_ADMIN");

    test_support::with_temp_netns("dpi-p1-", |peer1_ns| async move {
        test_support::with_temp_netns("dpi-p2-", |peer2_ns| async move {
            let mut provisioner = NetlinkNetworkProvisioner;

            let host1_name = test_support::unique_name("dpir10-");
            let peer1_name = test_support::unique_name("dpir11-");
            let host2_name = test_support::unique_name("dpir20-");
            let peer2_name = test_support::unique_name("dpir21-");

            // `create_veth` moves the peer end into `peer{1,2}_ns` as part
            // of creation; the host end stays in the default namespace.
            let pair1 = provisioner
                .create_veth(VethSpec {
                    host_ifname: host1_name.clone(),
                    peer_ifname: peer1_name.clone(),
                    peer_netns: peer1_ns.clone(),
                    host_mac: None,
                    peer_mac: None,
                })
                .await
                .expect("create_veth (pair 1) failed");
            let (mut host1, mut peer1) = (pair1.host, pair1.peer);

            let pair2 = provisioner
                .create_veth(VethSpec {
                    host_ifname: host2_name.clone(),
                    peer_ifname: peer2_name.clone(),
                    peer_netns: peer2_ns.clone(),
                    host_mac: None,
                    peer_mac: None,
                })
                .await
                .expect("create_veth (pair 2) failed");
            let (mut host2, mut peer2) = (pair2.host, pair2.peer);

            let host1_ip = Ipv4Addr::new(192, 168, 21, 1);
            let peer1_ip = Ipv4Addr::new(192, 168, 21, 21);
            let host2_ip = Ipv4Addr::new(192, 168, 22, 1);
            let peer2_ip = Ipv4Addr::new(192, 168, 22, 22);

            host1.set_up().await.expect("host1 set_up failed");
            host1
                .set_addr(v4(host1_ip, 24))
                .await
                .expect("host1 set_addr failed");
            host2.set_up().await.expect("host2 set_up failed");
            host2
                .set_addr(v4(host2_ip, 24))
                .await
                .expect("host2 set_addr failed");

            peer1.set_up().await.expect("peer1 set_up failed");
            peer1
                .set_addr(v4(peer1_ip, 24))
                .await
                .expect("peer1 set_addr failed");
            peer1
                .add_gateway(host1_ip)
                .await
                .expect("peer1 add_gateway failed");

            peer2.set_up().await.expect("peer2 set_up failed");
            peer2
                .set_addr(v4(peer2_ip, 24))
                .await
                .expect("peer2 set_addr failed");
            peer2
                .add_gateway(host2_ip)
                .await
                .expect("peer2 add_gateway failed");

            // Host ends live in the default namespace now, so this runs
            // directly -- no namespace switch needed.
            std::fs::write("/proc/sys/net/ipv4/ip_forward", b"1")
                .expect("enabling ip_forward failed");
            std::fs::write(
                format!("/proc/sys/net/ipv4/conf/{host1_name}/forwarding"),
                b"1",
            )
            .expect("enabling forwarding on host1 failed");
            std::fs::write(
                format!("/proc/sys/net/ipv4/conf/{host2_name}/forwarding"),
                b"1",
            )
            .expect("enabling forwarding on host2 failed");

            // A socket's namespace is fixed at creation time and stays
            // that way for its whole life, regardless of which
            // namespace the calling thread is in when it's later used
            // -- so both sockets get created inside their respective
            // peer namespaces here, then driven directly from the
            // ambient (ignoring-namespaces) code below.
            let listener = Netns::open_path(&peer1_ns)
                .unwrap()
                .run(move |_handle| async move {
                    UdpSocket::bind((peer1_ip, 0)).map_err(InfraError::Runtime)
                })
                .await
                .expect("failed to bind listening socket on peer1");
            listener
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set_read_timeout failed");
            let listener_addr = listener.local_addr().expect("local_addr failed");

            let sender = Netns::open_path(&peer2_ns)
                .unwrap()
                .run(move |_handle| async move {
                    UdpSocket::bind((peer2_ip, 0)).map_err(InfraError::Runtime)
                })
                .await
                .expect("failed to bind sending socket on peer2");

            let payload = b"hello from peer2";
            sender
                .send_to(payload, listener_addr)
                .expect("send_to failed");

            let mut buf = [0u8; 64];
            let (n, from) = listener.recv_from(&mut buf).expect("recv_from failed");
            assert_eq!(&buf[..n], payload);
            assert_eq!(from.ip(), IpAddr::V4(peer2_ip));

            // Not strictly required -- host1/host2 live in the
            // `unshare_self`d namespace, which vanishes with this test's
            // thread regardless -- but deterministic and cheap, and
            // deleting each host also deletes its peer, so peer1_ns/
            // peer2_ns end up empty before their own `with_temp_netns`
            // guards remove them.
            host1.delete().await.expect("host1 cleanup delete failed");
            host2.delete().await.expect("host2 cleanup delete failed");
        })
        .await;
    })
    .await;
}
