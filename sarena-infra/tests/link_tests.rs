use std::net::{IpAddr, Ipv4Addr};

use futures::TryStreamExt;
use netlink_packet_route::route::{RouteAddress, RouteAttribute};
use rtnetlink::RouteMessageBuilder;
use sarena_infra::{
    InfraError, Link, MacAddress, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, VethSpec,
    netlink_link::LinkKind, test_support,
};

/// Raw netlink check: does namespace `ns` have address `ip/prefix_len`
/// configured on interface `ifindex`? `sarena-infra` has no address-query
/// API yet (only [`Link::set_addr`]), so this queries directly.
async fn has_address(ns: &str, ifindex: u32, ip: Ipv4Addr, prefix_len: u8) -> bool {
    Netns::open(ns)
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
async fn has_default_gateway(ns: &str, ifindex: u32, gateway: Ipv4Addr) -> bool {
    Netns::open(ns)
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

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn veth_pair_create_and_configure() {
    test_support::with_temp_netns("dpi-veth-", |ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
        let name = test_support::unique_name("dpiv0-");
        let peer_name = test_support::unique_name("dpiv1-");
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
        let (mut host, peer) = (pair.host, pair.peer);
        host.set_ns(&ns).await.expect("link_setns failed for host");

        // Both ends have left the default namespace.
        assert!(provisioner.get_link(&name).await.is_err());
        assert!(provisioner.get_link(&peer_name).await.is_err());

        assert_eq!(host.name, name);
        assert_eq!(peer.name, peer_name);
        assert_eq!(host.kind, LinkKind::Veth);
        assert_eq!(peer.kind, LinkKind::Veth);
        assert!(!host.is_up(), "veth ends should start down");

        host.set_up().await.expect("link_set_up failed");
        let refreshed = provisioner.get_link_in_ns(&ns, &name).await.unwrap();
        assert!(refreshed.is_up());

        host.set_mtu(1400).await.expect("link_set_mtu failed");
        let refreshed = provisioner.get_link_in_ns(&ns, &name).await.unwrap();
        assert_eq!(refreshed.mtu, Some(1400));

        let mac = MacAddress::parse("02:00:00:00:00:01").expect("valid MAC literal");
        host.set_mac(mac).await.expect("link_set_mac failed");
        let refreshed = provisioner.get_link_in_ns(&ns, &name).await.unwrap();
        assert_eq!(refreshed.mac, Some(mac));

        host.set_down().await.expect("link_set_down failed");
        let refreshed = provisioner.get_link_in_ns(&ns, &name).await.unwrap();
        assert!(!refreshed.is_up());

        host.delete().await.expect("delete failed");
        assert!(provisioner.get_link_in_ns(&ns, &name).await.is_err());

        // Deleting one end of a veth pair deletes both.
        assert!(provisioner.get_link_in_ns(&ns, &peer_name).await.is_err());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn veth_pair_create_and_configure_default_ns() {
    // The peer end still needs *some* namespace -- `create_veth` always
    // moves it, mirroring how the router actually wires up a veth pair --
    // so this uses a throwaway namespace for the peer while keeping the
    // host end (the one under test here) in the default namespace.
    test_support::with_temp_netns("dpid-peer-", |peer_ns| async move {
        let mut provisioner = NetlinkNetworkProvisioner;
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

        assert_eq!(host.name, name);
        assert_eq!(peer.name, peer_name);
        assert_eq!(host.kind, LinkKind::Veth);
        assert_eq!(peer.kind, LinkKind::Veth);
        assert!(!host.is_up(), "veth ends should start down");

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
        let names: Vec<_> = links.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&name.as_str()));
        // The peer end lives in `peer_ns`, not the default namespace.
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
                .get_link_in_ns(&peer_ns, &peer_name)
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
        host.set_ns(&ns).await.expect("link_setns failed for host");

        host.rename(&to_name).await.expect("rename failed");

        assert!(provisioner.get_link_in_ns(&ns, &from_name).await.is_err());
        assert!(provisioner.get_link_in_ns(&ns, &to_name).await.is_ok());
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn link_setns_moves_only_the_moved_end() {
    // Nested (rather than manually created/deleted) so that a panicking
    // assertion below still tears both namespaces down via their drop
    // guards, instead of leaking them for the next run to trip over.
    test_support::with_temp_netns("dpi-mva-", |ns_a| async move {
        test_support::with_temp_netns("dpi-mvb-", |ns_b| async move {
            let mut provisioner = NetlinkNetworkProvisioner;
            let host_name = test_support::unique_name("dpimv0-");
            let peer_name = test_support::unique_name("dpimv1-");
            // `create_veth` moves the peer into `ns_a` as part of creation.
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
            let (mut host_end, mut peer) = (pair.host, pair.peer);
            host_end
                .set_ns(&ns_a)
                .await
                .expect("link_setns failed for host_end");

            // Both ends have left the default namespace.
            assert!(provisioner.get_link(&host_name).await.is_err());
            assert!(provisioner.get_link(&peer_name).await.is_err());

            peer.set_ns(&ns_b).await.expect("link_setns failed");

            // The moved end is gone from ns_a and present in ns_b.
            assert!(provisioner.get_link_in_ns(&ns_a, &peer_name).await.is_err());
            assert!(provisioner.get_link_in_ns(&ns_b, &peer_name).await.is_ok());

            // The end that stayed behind is still exactly where it was.
            let still_there = provisioner
                .get_link_in_ns(&ns_a, &host_name)
                .await
                .expect("host_end should still be in ns_a");
            assert_eq!(still_there.index, host_end.index);
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
        host.set_ns(&ns).await.expect("link_setns failed for host");

        let links = provisioner
            .list_links_in_ns(&ns)
            .await
            .expect("list_links failed");
        let names: Vec<_> = links.iter().map(|l| l.name.as_str()).collect();

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
        host.set_ns(&ns).await.expect("link_setns failed for host");
        host.set_up().await.expect("link_set_up failed");

        let ip = Ipv4Addr::new(10, 99, 0, 1);
        host.set_addr(ip, 24).await.expect("set_addr failed");
        assert!(has_address(&ns, host.ifindex(), ip, 24).await);

        // Idempotent: setting the exact same address again must not error
        // -- this is exactly what `.replace()` in `link_set_addr_impl` buys
        // us.
        host.set_addr(ip, 24).await.expect("repeat set_addr failed");
        assert!(has_address(&ns, host.ifindex(), ip, 24).await);

        let gateway = Ipv4Addr::new(10, 99, 0, 254);
        host.add_gateway(gateway).await.expect("add_gateway failed");
        assert!(has_default_gateway(&ns, host.ifindex(), gateway).await);

        // Idempotent for the same reason: replacing the default route with
        // a different gateway must succeed rather than fail with "already
        // exists".
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
    // Interface names are capped at IFNAMSIZ-1 (15) bytes, unlike namespace
    // names -- keep this prefix short enough that `unique_name` can't push
    // it over the limit (which would make the kernel reject the lookup's
    // IFLA_IFNAME attribute with ERANGE instead of reporting "not found").
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
    let provisioner = NetlinkNetworkProvisioner;
    let missing = test_support::unique_name("dpi-missing-");

    let err = provisioner
        .list_links_in_ns(&missing)
        .await
        .expect_err("list_links_in_ns should fail for a namespace that was never created");
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

        let err = host
            .set_ns(&missing_ns)
            .await
            .expect_err("set_ns to a namespace that doesn't exist should fail");
        assert!(matches!(err, InfraError::OpenNamespace { name, .. } if name == missing_ns));

        // The failed move must not have taken effect -- the link is still
        // exactly where it was, in the default namespace.
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
        host.set_ns(&ns).await.expect("link_setns failed for host");

        // The peer end already occupies `peer_name` in the same namespace,
        // so renaming the host to it must fail, and must not partially
        // apply.
        assert!(host.rename(&peer_name).await.is_err());
        assert!(provisioner.get_link_in_ns(&ns, &name).await.is_ok());
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
        host.set_ns(&ns).await.expect("link_setns failed for host");

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

        // The host end deliberately stays in the default namespace here
        // (not moved away), so its name still collides with the second
        // attempt below.
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
