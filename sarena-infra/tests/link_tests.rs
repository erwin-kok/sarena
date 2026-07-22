use sarena_infra::{
    Link, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner, VethSpec,
    netlink_link::LinkKind, test_support,
};

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
