use sarena_infra::{
    InfraError, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, test_support,
};

#[tokio::test]
#[ignore = "requires CAP_SYS_ADMIN and a writable /run/netns"]
async fn create_and_delete_round_trip() {
    let name = test_support::unique_name("dpi-cr-");

    Netns::create(&name).await.expect("create_netns failed");

    let listed = Netns::list().expect("list_netns failed");
    assert!(
        listed.contains(&name),
        "expected {name:?} in listing {listed:?}"
    );

    // create_netns should have brought lo up automatically.
    let provisioner = NetlinkNetworkProvisioner;
    let lo = provisioner
        .get_link_in_ns(&name, "lo")
        .await
        .expect("failed to fetch lo in freshly created namespace");
    assert!(
        lo.is_up(),
        "lo should already be up in a namespace created by create_netns"
    );

    Netns::delete(&name).expect("delete_netns failed");

    let listed_after = Netns::list().expect("list_netns failed");
    assert!(
        !listed_after.contains(&name),
        "namespace {name:?} should be gone after delete_netns"
    );
}

#[tokio::test]
#[ignore = "requires CAP_SYS_ADMIN and a writable /run/netns"]
async fn create_rejects_duplicate_name() {
    let name = test_support::unique_name("dpi-dup-");
    Netns::create(&name)
        .await
        .expect("first create_netns should succeed");

    let second_attempt = Netns::create(&name).await;
    assert!(
        matches!(second_attempt, Err(InfraError::NamespaceExists(_))),
        "expected NamespaceExists, got {second_attempt:?}"
    );

    // The failed duplicate attempt must not have clobbered or removed the
    // namespace created by the first call.
    let listed = Netns::list().expect("list_netns failed");
    assert!(listed.contains(&name));

    Netns::delete(&name).expect("cleanup failed");
}

#[tokio::test]
#[ignore = "requires CAP_SYS_ADMIN and a writable /run/netns"]
async fn delete_nonexistent_namespace_errors() {
    let name = test_support::unique_name("dpi-missing-");
    let result = Netns::delete(&name);
    assert!(matches!(result, Err(InfraError::NamespaceNotFound(_))));
}

#[tokio::test]
#[ignore = "requires CAP_SYS_ADMIN and a writable /run/netns"]
async fn run_works_across_independent_namespaces() {
    // Regression guard for `Netns::run`'s dedicated-thread design: running
    // a closure in one namespace must not leave any thread-local state that
    // affects a *separate* `Netns::run` call against a different
    // namespace, since each call gets its own one-shot thread.
    let a = test_support::unique_name("dpi-ra-");
    let b = test_support::unique_name("dpi-rb-");
    Netns::create(&a).await.expect("create ns a");
    Netns::create(&b).await.expect("create ns b");

    for ns_name in [&a, &b] {
        let handle_ns = Netns::open(ns_name).expect("open namespace");
        handle_ns
            .run(|handle| async move {
                // Confirm we can actually talk netlink from inside; fetching
                // lo should always succeed in a namespace create_netns made.
                use futures::TryStreamExt;
                handle
                    .link()
                    .get()
                    .match_name("lo".to_owned())
                    .execute()
                    .try_next()
                    .await
                    .map_err(InfraError::Netlink)?
                    .ok_or_else(|| InfraError::LinkNotFound("lo".to_owned()))?;
                Ok::<_, InfraError>(())
            })
            .await
            .unwrap_or_else(|e| panic!("Netns::run failed in {ns_name:?}: {e}"));
    }

    Netns::delete(&a).expect("cleanup a");
    Netns::delete(&b).expect("cleanup b");
}
