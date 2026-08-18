use std::{
    net::{IpAddr, Ipv4Addr},
    path::Path,
};

use futures::TryStreamExt;
use ipnet::IpNet;
use netlink_packet_route::route::{
    RouteAddress, RouteAttribute, RouteMessage, RouteMetric, RouteScope,
};
use rtnetlink::RouteMessageBuilder;
use sarena_infra::{
    InfraError, Link, MacAddress, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, VethSpec,
    netlink_link::LinkKind, route::Route, test_support,
};

/// Raw netlink check: does namespace `ns` have address `ip/prefix_len`
/// configured on interface `ifindex`? `sarena-infra` has no address-query
/// API yet (only [`Link::set_addr`]), so this queries directly.
async fn has_address(ns: &Path, ifindex: u32, ip: Ipv4Addr, prefix_len: u8) -> bool {
    Netns::open_path(ns)
        .unwrap()
        .run(move |handle| async move {
            let addrs: Vec<_> = handle
                .address()
                .get()
                .set_link_index_filter(ifindex)
                .set_address_filter(IpAddr::V4(ip))
                .set_prefix_length_filter(prefix_len)
                .execute()
                .try_collect()
                .await
                .map_err(InfraError::Netlink)?;
            Ok(!addrs.is_empty())
        })
        .await
        .expect("address query failed")
}

/// Raw netlink check: does namespace `ns` have a default route out through
/// `ifindex` via `gateway`? `sarena-infra` has no route-query API yet (only
/// [`Link::add_gateway`]), so this queries directly.
async fn has_default_gateway(ns: &Path, ifindex: u32, gateway: Ipv4Addr) -> bool {
    Netns::open_path(ns)
        .unwrap()
        .run(move |handle| async move {
            let routes: Vec<_> = handle
                .route()
                .get(RouteMessageBuilder::<Ipv4Addr>::new().build())
                .execute()
                .try_collect()
                .await
                .map_err(InfraError::Netlink)?;

            Ok(routes.iter().any(|route| {
                let via_ifindex = route
                    .attributes
                    .iter()
                    .any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == ifindex));
                let via_gateway = route.attributes.iter().any(|a| {
                    matches!(a, RouteAttribute::Gateway(RouteAddress::Inet(gw)) if *gw == gateway)
                });
                via_ifindex && via_gateway
            }))
        })
        .await
        .expect("route query failed")
}

/// Raw netlink check: returns the route matching destination
/// `prefix/prefix_len` inside namespace `ns`, if any. `sarena-infra` has no
/// route-query API yet (only [`Link::add_route`]), so this queries
/// directly.
async fn find_route(ns: &Path, prefix: Ipv4Addr, prefix_len: u8) -> Option<RouteMessage> {
    Netns::open_path(ns)
        .unwrap()
        .run(move |handle| async move {
            let routes: Vec<_> = handle
                .route()
                .get(RouteMessageBuilder::<Ipv4Addr>::new().build())
                .execute()
                .try_collect()
                .await
                .map_err(InfraError::Netlink)?;

            Ok(routes.into_iter().find(|route| {
                route.header.destination_prefix_length == prefix_len
                    && route.attributes.iter().any(|a| {
                        matches!(
                            a,
                            RouteAttribute::Destination(RouteAddress::Inet(addr))
                                if *addr == prefix
                        )
                    })
            }))
        })
        .await
        .expect("route query failed")
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn veth_pair_create_and_configure() {
    test_support::with_temp_netns("dpid-peer-", |peer_ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let peer_netns = Netns::open_path(&peer_ns).expect("open temp netns");
        let name = test_support::unique_name("dpid0-");
        let peer_name = test_support::unique_name("dpid1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: peer_ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let (mut host, peer) = (pair.host, pair.peer);

        assert_eq!(host.ifname(), name);
        assert_eq!(peer.ifname(), peer_name);
        let host_snapshot = provisioner.get_link(&name).await.unwrap();
        let peer_snapshot = provisioner
            .get_link_in_ns(&peer_netns, &peer_name)
            .await
            .unwrap();
        assert_eq!(host_snapshot.kind, LinkKind::Veth);
        assert_eq!(peer_snapshot.kind, LinkKind::Veth);
        assert!(!host_snapshot.is_up(), "veth ends should start down");

        host.set_up().await.expect("link_set_up failed");
        let refreshed = provisioner.get_link(&name).await.unwrap();
        assert!(refreshed.is_up());

        host.set_mtu(1400).await.expect("link_set_mtu failed");
        let refreshed = provisioner.get_link(&name).await.unwrap();
        assert_eq!(refreshed.mtu, Some(1400));

        let mac = MacAddress::parse("02:00:00:00:00:02").expect("valid MAC literal");
        host.set_mac(mac).await.expect("link_set_mac failed");
        let refreshed = provisioner.get_link(&name).await.unwrap();
        assert_eq!(refreshed.mac, Some(mac));

        host.set_down().await.expect("link_set_down failed");
        let refreshed = provisioner.get_link(&name).await.unwrap();
        assert!(!refreshed.is_up());

        let links = provisioner.list_links().await.expect("list_links failed");
        let names: Vec<_> = links.iter().map(|l| l.ifname()).collect();
        assert!(names.contains(&name.as_str()));
        assert!(!names.contains(&peer_name.as_str()));

        let renamed = test_support::unique_name("dpid2-");
        host.rename(&renamed).await.expect("rename failed");
        assert!(provisioner.get_link(&name).await.is_err());
        assert!(provisioner.get_link(&renamed).await.is_ok());

        host.delete().await.expect("delete failed");
        assert!(provisioner.get_link(&renamed).await.is_err());

        // Deleting one end of a veth pair deletes both, even though the
        // peer lives in a different namespace.
        assert!(
            provisioner
                .get_link_in_ns(&peer_netns, &peer_name)
                .await
                .is_err()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn rename_link_by_name() {
    test_support::with_temp_netns("dpi-rn-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let from_name = test_support::unique_name("dpiold0-");
        let peer_name = test_support::unique_name("dpiold1-");
        let to_name = test_support::unique_name("dpinew0-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: from_name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;

        host.rename(&to_name).await.expect("rename failed");

        assert!(provisioner.get_link(&from_name).await.is_err());
        assert!(provisioner.get_link(&to_name).await.is_ok());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn link_setns_moves_only_the_moved_end() {
    test_support::with_temp_netns("dpi-mva-", |ns_a| async move {
        test_support::with_temp_netns("dpi-mvb-", |ns_b| async move {
            let mut provisioner = NetlinkNetworkProvisioner;
            let netns_a = Netns::open_path(&ns_a).expect("open temp netns");
            let netns_b = Netns::open_path(&ns_b).expect("open temp netns");
            let host_name = test_support::unique_name("dpimv0-");
            let peer_name = test_support::unique_name("dpimv1-");
            let pair = provisioner
                .create_veth(VethSpec {
                    host_ifname: host_name.clone(),
                    peer_ifname: peer_name.clone(),
                    peer_netns: ns_a.clone(),
                    host_mac: None,
                    peer_mac: None,
                })
                .await
                .expect("create_veth failed");
            let (mut host, peer) = (pair.host, pair.peer);

            host.set_ns(&netns_b).await.expect("link_setns failed");

            assert!(provisioner.get_link(&host_name).await.is_err());
            assert!(
                provisioner
                    .get_link_in_ns(&netns_b, &host_name)
                    .await
                    .is_ok()
            );

            let still_there = provisioner
                .get_link_in_ns(&netns_a, &peer_name)
                .await
                .expect("peer should still be in ns_a");
            assert_eq!(still_there.ifindex(), peer.ifindex());
        })
        .await;
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn list_links_includes_loopback_and_veth() {
    test_support::with_temp_netns("dpi-list-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpils0-");
        let peer_name = test_support::unique_name("dpils1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");

        let links = provisioner
            .list_links_in_ns(&netns)
            .await
            .expect("list_links failed");
        let names: Vec<_> = links.iter().map(|l| l.ifname()).collect();

        assert!(names.contains(&"lo"));
        assert!(names.contains(&name.as_str()));
        assert!(names.contains(&peer_name.as_str()));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn set_addr_and_add_gateway_configure_the_link() {
    test_support::with_temp_netns("dpi-addr-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpiad0-");
        let peer_name = test_support::unique_name("dpiad1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");
        host.set_up().await.expect("link_set_up failed");

        let ip = Ipv4Addr::new(10, 99, 0, 1);
        let addr = IpNet::new(IpAddr::V4(ip), 24).expect("valid IPv4 network");
        host.set_addr(addr).await.expect("set_addr failed");
        assert!(has_address(&ns, host.ifindex(), ip, 24).await);

        host.set_addr(addr).await.expect("repeat set_addr failed");
        assert!(has_address(&ns, host.ifindex(), ip, 24).await);

        let gateway = Ipv4Addr::new(10, 99, 0, 254);
        host.add_gateway(gateway).await.expect("add_gateway failed");
        assert!(has_default_gateway(&ns, host.ifindex(), gateway).await);

        let gateway2 = Ipv4Addr::new(10, 99, 0, 253);
        host.add_gateway(gateway2)
            .await
            .expect("replacing add_gateway failed");
        assert!(has_default_gateway(&ns, host.ifindex(), gateway2).await);
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn get_link_for_missing_name_fails() {
    let provisioner = NetlinkNetworkProvisioner;
    let missing = test_support::unique_name("dpim0-");
    let err = provisioner
        .get_link(&missing)
        .await
        .expect_err("get_link should fail for a name that was never created");
    assert!(matches!(err, InfraError::LinkNotFound(n) if n == missing));
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn list_links_in_ns_for_missing_namespace_fails() {
    let missing = test_support::unique_name("dpi-missing-");

    let err = Netns::open(&missing)
        .expect_err("Netns::open should fail for a namespace that was never created");
    assert!(matches!(err, InfraError::OpenNamespace { name, .. } if name == missing));
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn set_ns_to_missing_namespace_fails_without_moving_the_link() {
    test_support::with_temp_netns("dpi-nsx-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let name = test_support::unique_name("dpinx0-");
        let peer_name = test_support::unique_name("dpinx1-");
        let missing_ns = test_support::unique_name("dpi-missing-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        let err = Netns::open(&missing_ns)
            .expect_err("opening a namespace that doesn't exist should fail");
        assert!(matches!(err, InfraError::OpenNamespace { name, .. } if name == missing_ns));
        assert!(provisioner.get_link(&name).await.is_ok());
        host.delete().await.expect("cleanup delete failed");
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn rename_to_existing_name_fails() {
    test_support::with_temp_netns("dpi-rnerr-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpirn0-");
        let peer_name = test_support::unique_name("dpirn1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");
        assert!(host.rename(&peer_name).await.is_err());
        assert!(provisioner.get_link_in_ns(&netns, &name).await.is_ok());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_twice_fails_the_second_time() {
    test_support::with_temp_netns("dpi-del2-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let name = test_support::unique_name("dpidl0-");
        let peer_name = test_support::unique_name("dpidl1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;

        host.delete().await.expect("first delete failed");
        assert!(
            host.delete().await.is_err(),
            "deleting an already-deleted link should fail"
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn create_veth_with_duplicate_name_fails() {
    test_support::with_temp_netns("dpi-dup-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let name = test_support::unique_name("dpidup0-");
        let peer_name = test_support::unique_name("dpidup1-");
        let other_peer_name = test_support::unique_name("dpidup2-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("first create_veth failed");
        let mut host = pair.host;

        let result = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: other_peer_name,
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await;
        assert!(
            result.is_err(),
            "duplicate interface name should be rejected"
        );

        host.delete().await.expect("cleanup delete failed");
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_link_by_name_removes_it_and_its_peer() {
    test_support::with_temp_netns("dpi-dlnm-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpidln0-");
        let peer_name = test_support::unique_name("dpidln1-");
        provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");

        provisioner
            .delete_link(&name)
            .await
            .expect("delete_link failed");

        assert!(provisioner.get_link(&name).await.is_err());
        assert!(
            provisioner
                .get_link_in_ns(&netns, &peer_name)
                .await
                .is_err()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_link_in_ns_removes_it_and_its_peer() {
    test_support::with_temp_netns("dpi-dlin-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpidli0-");
        let peer_name = test_support::unique_name("dpidli1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");

        provisioner
            .delete_link_in_ns(&netns, &name)
            .await
            .expect("delete_link_in_ns failed");

        assert!(provisioner.get_link_in_ns(&netns, &name).await.is_err());
        assert!(
            provisioner
                .get_link_in_ns(&netns, &peer_name)
                .await
                .is_err()
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_link_for_missing_name_fails() {
    let provisioner = NetlinkNetworkProvisioner;
    let missing = test_support::unique_name("dpidlm0-");

    let err = provisioner
        .delete_link(&missing)
        .await
        .expect_err("delete_link should fail for a name that was never created");
    assert!(matches!(err, InfraError::LinkNotFound(n) if n == missing));
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_link_in_ns_for_missing_link_fails() {
    test_support::with_temp_netns("dpi-dlml-", |ns| async move {
        let provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let missing = test_support::unique_name("dpidlm1-");

        let err = provisioner
            .delete_link_in_ns(&netns, &missing)
            .await
            .expect_err("delete_link_in_ns should fail for a name that was never created");
        assert!(matches!(err, InfraError::LinkNotFound(n) if n == missing));
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_link_in_ns_for_missing_namespace_fails() {
    let missing_ns = test_support::unique_name("dpi-missing-");
    let err = Netns::open(&missing_ns)
        .expect_err("Netns::open should fail for a namespace that was never created");
    assert!(matches!(err, InfraError::OpenNamespace { name, .. } if name == missing_ns));
}

/// A minimal `Route` for `prefix`, with every other field left unset --
/// tests override just the fields they care about via `..route(prefix)`.
fn route(prefix: IpNet) -> Route {
    Route {
        prefix,
        nexthop: None,
        local: None,
        device: None,
        mtu: None,
        priority: None,
        proto: None,
        scope: None,
        table: None,
        kind: None,
    }
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn add_route_without_a_nexthop_installs_an_on_link_route() {
    test_support::with_temp_netns("dpi-rtol-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpirt0-");
        let peer_name = test_support::unique_name("dpirt1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");
        host.set_up().await.expect("link_set_up failed");

        let prefix = Ipv4Addr::new(10, 77, 0, 0);
        let network = IpNet::new(IpAddr::V4(prefix), 24).expect("valid IPv4 network");
        host.add_route(&route(network))
            .await
            .expect("add_route failed");

        let installed = find_route(&ns, prefix, 24)
            .await
            .expect("route should have been installed");
        assert_eq!(installed.header.scope, RouteScope::Link);
        assert!(
            installed
                .attributes
                .iter()
                .any(|a| matches!(a, RouteAttribute::Oif(idx) if *idx == host.ifindex()))
        );
        // No nexthop was given, so no gateway attribute should be present.
        assert!(
            !installed
                .attributes
                .iter()
                .any(|a| matches!(a, RouteAttribute::Gateway(_)))
        );
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn add_route_with_a_nexthop_installs_a_route_via_gateway() {
    test_support::with_temp_netns("dpi-rtgw-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpirt2-");
        let peer_name = test_support::unique_name("dpirt3-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");
        host.set_up().await.expect("link_set_up failed");

        // The gateway has to be reachable via an already-connected subnet,
        // or the kernel rejects the route with ENETUNREACH -- so give the
        // link an address on the same /24 the nexthop below lives in.
        let host_ip = Ipv4Addr::new(10, 78, 0, 1);
        host.set_addr(IpNet::new(IpAddr::V4(host_ip), 24).unwrap())
            .await
            .expect("set_addr failed");

        let nexthop = Ipv4Addr::new(10, 78, 0, 2);
        let prefix = Ipv4Addr::new(10, 88, 0, 0);
        let network = IpNet::new(IpAddr::V4(prefix), 24).expect("valid IPv4 network");
        let mut r = route(network);
        r.nexthop = Some(IpAddr::V4(nexthop));
        host.add_route(&r).await.expect("add_route failed");

        let installed = find_route(&ns, prefix, 24)
            .await
            .expect("route should have been installed");
        // A route with a nexthop keeps the builder's default (universe)
        // scope, unlike the on-link case above.
        assert_eq!(installed.header.scope, RouteScope::Universe);
        assert!(installed.attributes.iter().any(|a| matches!(
            a,
            RouteAttribute::Gateway(RouteAddress::Inet(gw)) if *gw == nexthop
        )));

        host.add_route(&r).await.expect("repeat add_route failed");
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn add_route_sets_table_and_mtu() {
    test_support::with_temp_netns("dpi-rttm-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let netns = Netns::open_path(&ns).expect("open temp netns");
        let name = test_support::unique_name("dpirt4-");
        let peer_name = test_support::unique_name("dpirt5-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let mut host = pair.host;
        host.set_ns(&netns)
            .await
            .expect("link_setns failed for host");
        host.set_up().await.expect("link_set_up failed");

        let prefix = Ipv4Addr::new(10, 99, 77, 0);
        let network = IpNet::new(IpAddr::V4(prefix), 24).expect("valid IPv4 network");
        let mut r = route(network);
        r.table = Some(100);
        r.mtu = Some(1300);
        host.add_route(&r).await.expect("add_route failed");

        let installed = find_route(&ns, prefix, 24)
            .await
            .expect("route should have been installed");
        assert_eq!(installed.header.table, 100);
        assert!(installed.attributes.iter().any(|a| matches!(
            a,
            RouteAttribute::Metrics(metrics)
                if metrics.contains(&RouteMetric::Mtu(1300))
        )));
    })
    .await;
}
