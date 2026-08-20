use std::{
    net::{Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::Arc,
};

use rscni_plugin::{
    error::Error,
    types::{self, Args, CNIResult, Interface, IpConfig},
};
use sarena_api_client::{ApiClient, Transport, attachment_id};
use sarena_api_types_v1::{
    endpoint::{self, Addressing},
    ipam,
};
use sarena_common_plugin::{
    ipam::{ipv4_routes, ipv6_routes},
    names::{endpoint_to_ifname, endpoint_to_temp_ifname},
};
use sarena_infra::{
    InfraError, InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, Netns,
    NetworkProvisioner as _, VethSpec,
    netlink_link::NetlinkLink,
    route::{Route, sort_by_mask_narrowest_first},
};
use tracing::{debug, info, instrument, warn};

use crate::{Res, args::ArgsSpec};

struct IpamLease<T: Transport + 'static> {
    client: Arc<ApiClient<T>>,
    ipv4: Option<ipam::ContainerAddressing>,
    ipv6: Option<ipam::ContainerAddressing>,
    keep: bool,
}

impl<T: Transport + 'static> IpamLease<T> {
    fn new(
        client: Arc<ApiClient<T>>,
        ipv4: Option<ipam::ContainerAddressing>,
        ipv6: Option<ipam::ContainerAddressing>,
    ) -> Self {
        Self {
            client,
            ipv4,
            ipv6,
            keep: false,
        }
    }
}

impl<T: Transport + 'static> Drop for IpamLease<T> {
    fn drop(&mut self) {
        if self.keep {
            return;
        }

        let client = Arc::clone(&self.client);
        let ipv4 = self.ipv4.take();
        let ipv6 = self.ipv6.take();

        tokio::spawn(async move {
            if let Some(addressing) = ipv4 {
                let ip = addressing.ip.clone();
                if let Err(e) = client.ipam().release(addressing.ip, addressing.pool).await {
                    warn!(ip, error = %e, "failed to release ipam lease");
                }
            }

            if let Some(addressing) = ipv6 {
                let ip = addressing.ip.clone();
                if let Err(e) = client.ipam().release(addressing.ip, addressing.pool).await {
                    warn!(ip, error = %e, "failed to release ipam lease");
                }
            }
        });
    }
}

#[instrument(skip_all, err)]
pub async fn add(args: Args, cni_args: ArgsSpec) -> Res<CNIResult> {
    let mut network_provisioner = NetlinkNetworkProvisioner;

    let Some(netns_path) = args.netns() else {
        return Err(Error::InvalidNetworkConfig("missing CNI_NETNS".to_string()));
    };
    let Some(ifname) = args.ifname() else {
        return Err(Error::InvalidNetworkConfig(
            "missing CNI_IFNAME".to_string(),
        ));
    };
    let Some(container_id) = args.container_id() else {
        return Err(Error::InvalidNetworkConfig(
            "missing CNI_CONTAINERID".to_string(),
        ));
    };
    if let Some(net_conf) = args.config() {
        let chained = net_conf.prev_result.is_some();
        if chained {
            return Err(Error::InvalidNetworkConfig(
                "chaining is currently not supported".to_string(),
            ));
        }
    }

    debug!(ifname, netns = %netns_path.display(), "deleting any stale link before ADD");
    let netns = Netns::open_path(netns_path).map_err(|e| {
        Error::InvalidNetworkConfig(format!(
            "could not open namespace {}: {e}",
            netns_path.display()
        ))
    })?;

    match network_provisioner.delete_link_in_ns(&netns, ifname).await {
        Ok(()) => debug!("stale link deleted"),
        // Nothing to clean up -- ADD is idempotent, so a link that
        // never existed (or was already removed) is a valid state, not
        // a failure.
        Err(InfraError::LinkNotFound(_)) => debug!("no stale link to delete"),
        Err(e) => {
            return Err(Error::InvalidNetworkConfig(format!(
                "could not delete link {ifname}: {e}"
            )));
        }
    }

    let api_client = ApiClient::new_default_client()
        .map(Arc::new)
        .map_err(|_| Error::PluginNotAvailable("DaemonDown".to_string()))?;

    let daemon_config = api_client
        .daemon()
        .get_config()
        .await
        .map_err(|_| Error::PluginNotAvailable("could not read config".to_string()))?;

    let pod_name = format!("{}/{}", cni_args.k8s_pod_namespace, cni_args.k8s_pod_name);
    let ipam_response = api_client
        .ipam()
        .allocate(pod_name.clone(), None)
        .await
        .map_err(|_| Error::PluginNotAvailable("could not allocate ip".to_string()))?;

    let mut lease = IpamLease::new(
        Arc::clone(&api_client),
        ipam_response.ipv4.clone(),
        ipam_response.ipv6.clone(),
    );

    if ipam_response.host_addressing.ipv4.is_none() && ipam_response.host_addressing.ipv6.is_none()
    {
        return Err(Error::PluginNotAvailable(
            "ipam should return valid host addressing".to_string(),
        ));
    }

    let (host, mut peer) = setup_veth(
        &mut network_provisioner,
        container_id,
        ifname,
        netns_path,
        daemon_config.device_mtu,
    )
    .await?;

    let mut routes: Vec<Route> = Vec::new();
    let mut endpoints: Vec<InterfaceAddress> = Vec::new();
    let mut ipv4_address: Option<endpoint::Addressing> = None;
    let mut ipv6_address: Option<endpoint::Addressing> = None;
    let mut cni_ips: Vec<IpConfig> = Vec::new();
    let mut cni_routes: Vec<types::Route> = Vec::new();

    if let Some(ipv4_host) = &ipam_response.host_addressing.ipv4
        && let Some(ipv4) = &ipam_response.ipv4
    {
        let gateway_ip = ipv4_host.parse::<Ipv4Addr>().map_err(|e| {
            Error::PluginNotAvailable(format!("could not parse host ipv4 address: {e}"))
        })?;
        let ipv4_routes = ipv4_routes(gateway_ip, daemon_config.route_mtu);
        routes.extend(ipv4_routes.clone());

        let endpoint_ip = ipv4.ip.parse::<InterfaceAddress>().map_err(|_| {
            Error::PluginNotAvailable("could not parse interface address".to_string())
        })?;
        endpoints.push(endpoint_ip);

        ipv4_address = Some(Addressing {
            ip: endpoint_ip.to_string(),
            pool: ipv4.pool.clone(),
        });

        cni_ips.push(types::IpConfig {
            interface: Some(1), // Must point to "cni_host_interface" index
            address: endpoint_ip.to_string(),
            gateway: Some(gateway_ip.to_string()),
        });

        cni_routes.extend(convert_to_cni_routes(ipv4_routes));
    }

    if let Some(ipv6_host) = &ipam_response.host_addressing.ipv6
        && let Some(ipv6) = &ipam_response.ipv6
    {
        let gateway_ip = ipv6_host.parse::<Ipv6Addr>().map_err(|e| {
            Error::PluginNotAvailable(format!("could not parse host ipv6 address: {e}"))
        })?;
        let ipv6_routes = ipv6_routes(gateway_ip, daemon_config.route_mtu);
        routes.extend(ipv6_routes.clone());

        let endpoint_ip = ipv6.ip.parse::<InterfaceAddress>().map_err(|_| {
            Error::PluginNotAvailable("could not parse interface address".to_string())
        })?;
        endpoints.push(endpoint_ip);

        ipv6_address = Some(Addressing {
            ip: endpoint_ip.to_string(),
            pool: ipv6.pool.clone(),
        });

        cni_ips.push(types::IpConfig {
            interface: Some(1), // Must point to "cni_host_interface" index
            address: endpoint_ip.to_string(),
            gateway: Some(gateway_ip.to_string()),
        });

        cni_routes.extend(convert_to_cni_routes(ipv6_routes));
    }

    configure_iface(&mut peer, &endpoints, &mut routes).await?;

    let ep = endpoint::EndpointCreateRequest {
        k8s_namespace: cni_args.k8s_pod_namespace.clone(),
        k8s_pod_name: cni_args.k8s_pod_name.clone(),
        k8s_uid: cni_args.k8s_pod_uid.clone(),
        container_id: container_id.to_string(),
        container_iface_name: ifname.to_string(),
        container_mac: peer.mac().to_string(),
        host_mac: host.mac().to_string(),
        host_iface_index: host.ifindex(),
        host_iface_name: host.ifname().to_string(),
        ipv4: ipv4_address,
        ipv6: ipv6_address,
    };
    let _new_ep = api_client
        .endpoint()
        .create(&attachment_id(container_id, ifname), &ep)
        .await
        .map_err(|_| Error::PluginNotAvailable("could not create endpoint".to_string()))?;

    lease.keep = true;

    let cni_host_interface = Interface {
        name: host.ifname().to_string(),
        mac: host.mac().to_string(),
        mtu: None,
        sandbox: None,
        socket_path: None,
        pci_id: None,
    };

    let cni_pod_interface = Interface {
        name: ifname.to_string(),
        mac: peer.mac().to_string(),
        mtu: None,
        sandbox: Some(netns_path.to_string_lossy().to_string()),
        socket_path: None,
        pci_id: None,
    };

    let result = CNIResult {
        // If the order of interfaces changes, also cahange index in convert_to_cni_ip_config
        interfaces: vec![cni_host_interface, cni_pod_interface],
        ips: cni_ips,
        routes: cni_routes,
        ..Default::default()
    };

    let json = serde_json::to_string_pretty(&result).unwrap();
    info!("CNI ADD result: {json}");

    Ok(result)
}

async fn setup_veth(
    network_provisioner: &mut NetlinkNetworkProvisioner,
    container_id: &str,
    ifname: &str,
    peer_netns_path: &Path,
    device_mtu: u32,
) -> Res<(NetlinkLink, NetlinkLink)> {
    let endpoint_id = format!("{container_id}:{ifname}");
    let lxc_ifname = endpoint_to_ifname(&endpoint_id);
    let tmp_peer_ifname = endpoint_to_temp_ifname(&endpoint_id);
    let host_mac = MacAddress::generate_rand();
    let lxc_mac = MacAddress::generate_rand();
    let pair = network_provisioner
        .create_veth(VethSpec {
            host_ifname: lxc_ifname.clone(),
            peer_ifname: tmp_peer_ifname,
            peer_netns: peer_netns_path.to_path_buf(),
            host_mac: Some(host_mac),
            peer_mac: Some(lxc_mac),
        })
        .await
        .map_err(|e| Error::InvalidNetworkConfig(format!("could not create veth pair: {e}")))?;
    let (mut host, mut peer) = (pair.host, pair.peer);

    peer.rename(ifname).await.map_err(|e| {
        Error::InvalidNetworkConfig(format!("could not rename peer interface: {e}"))
    })?;

    host.set_rp_filter(0)
        .await
        .map_err(|e| Error::InvalidNetworkConfig(format!("could not disable rp filter: {e}")))?;

    host.set_mtu(device_mtu).await.map_err(infra_err)?;
    peer.set_mtu(device_mtu).await.map_err(infra_err)?;

    host.set_up().await.map_err(infra_err)?;

    debug!(
        "new endpoint -- host_iface = {}, mac: {}",
        lxc_ifname, host_mac,
    );

    debug!(
        "new endpoint -- peer_iface = {}, mac: {}, index: {}",
        ifname,
        lxc_mac,
        peer.ifindex()
    );

    Ok((host, peer))
}

async fn configure_iface(
    peer: &mut NetlinkLink,
    interface_addresses: &[InterfaceAddress],
    routes: &mut Vec<Route>,
) -> Res<()> {
    peer.set_up().await.map_err(infra_err)?;

    for &interface_address in interface_addresses {
        peer.set_addr(interface_address).await.map_err(infra_err)?;
    }

    // Sort provided routes to make sure we apply any more specific
    // routes first which may be used as nexthops in wider routes
    sort_by_mask_narrowest_first(routes);

    for r in routes {
        peer.add_route(r)
            .await
            .map_err(|e| Error::InvalidNetworkConfig(format!("failed to add route: {e}")))?;
    }

    Ok(())
}

fn convert_to_cni_routes(routes: Vec<Route>) -> Vec<types::Route> {
    routes
        .iter()
        .map(|r| types::Route {
            dst: r.prefix.to_string(),
            mtu: r.mtu,
            gw: r.nexthop.map(|h| h.to_string()),
            advmss: None,
            priority: None,
            table: None,
            scope: None,
        })
        .collect()
}

fn infra_err(e: InfraError) -> Error {
    Error::InvalidNetworkConfig(e.to_string())
}
