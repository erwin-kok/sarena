use std::net::IpAddr;

use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    route::Route,
};
use sarena_shared::EndpointConfig;

use crate::{
    AyaBackend, EndpointConfigMap, EndpointKind, Loader, LoaderHandle, PinRoot,
    config::ControlPlaneConfig,
    error::Res,
    netlink::{SARENA_HOST, setup_host_device},
};

/// Where eBPF program/link state gets pinned. Shared between the loader
/// itself (below) and `sarena-services-endpoint-manager`'s
/// `LoaderEndpointService`, which is handed this same path rather than
/// hardcoding it -- keeps that crate from needing to know a specific
/// bpffs layout.
pub const PIN_ROOT: &str = "/sys/fs/bpf/sarena";
pub const DEFAULT_PIN_ROOT: &str = "/sys/fs/bpf/sarena";

pub async fn start_control_plane(
    config: &ControlPlaneConfig,
) -> Res<(LoaderHandle, std::thread::JoinHandle<()>)> {
    std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

    let backend = AyaBackend::new(format!("{PIN_ROOT}/globals"));

    let loader: Loader<AyaBackend> = Loader::new(backend, PIN_ROOT);

    let (loader_handle, loader_thread) = LoaderHandle::spawn(loader, 16);
    let shutdown_loader_handle = loader_handle.clone();

    loader_handle.load_global_maps().await?;

    let mut provisioner = NetlinkNetworkProvisioner;

    provisioner.set_rp_filter(0).await?;

    let (mut host, _) = setup_host_device(
        &mut provisioner,
        1500,
        InterfaceAddress {
            ip: config.internal_ip,
            prefix_len: 32,
        },
    )
    .await?;

    if let Some(prefix) = config.ipam_ipv4_subnet {
        let route = Route {
            nexthop: Some(config.internal_ip),
            local: Some(config.internal_ip),
            prefix,
            mtu: Some(1500),
            ..Default::default()
        };
        host.add_route(&route).await?;
    }

    loader_handle
        .add_endpoint(EndpointKind::Host, SARENA_HOST)
        .await?;

    set_endpoint_config(host.mac(), config.internal_ip);

    Ok((shutdown_loader_handle, loader_thread))
}

fn set_endpoint_config(host_mac: MacAddress, addr: IpAddr) {
    let ipv4 = match addr {
        IpAddr::V4(addr) => addr,
        IpAddr::V6(_) => panic!("expected IPv4 address"),
    };
    EndpointConfigMap::for_link(&PinRoot::new(PIN_ROOT), SARENA_HOST)
        .expect("open endpoint_config map")
        .set(EndpointConfig {
            mac: host_mac.0,
            ipv4,
        })
        .expect("set endpoint config");
}
