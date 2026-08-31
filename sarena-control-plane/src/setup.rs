use std::{fs, net::IpAddr, sync::Arc};

use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, route::Route,
};
use sarena_loader::{AyaBackend, EndpointConfigMap, EndpointKind, Loader, LoaderHandle, PinRoot};
use sarena_shared::EndpointConfig;

use crate::{
    AppState, Res,
    config::ControlPlaneConfig,
    netlink::{SARENA_HOST, setup_host_device},
};

/// Where eBPF program/link state gets pinned. Shared between the loader
/// itself (below) and `sarena-services-endpoint-manager`'s
/// `LoaderEndpointService`, which is handed this same path rather than
/// hardcoding it -- keeps that crate from needing to know a specific
/// bpffs layout.
const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

pub struct ControlPlane {
    pub config: ControlPlaneConfig,
}

impl ControlPlane {
    pub fn new(config: ControlPlaneConfig) -> Self {
        Self { config }
    }

    pub async fn start(&self) -> Res<AppState> {
        let _ = fs::remove_dir_all(PIN_ROOT);

        std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

        let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
        let backend = AyaBackend::new(
            format!("{dir}/sarena-ebpf-programs.o"),
            format!("{PIN_ROOT}/globals"),
        );
        let loader: Loader<AyaBackend> = Loader::new(backend, PIN_ROOT);

        let loader_handle = LoaderHandle::spawn(loader, 16);

        loader_handle.load_global_maps().await?;

        let mut provisioner = NetlinkNetworkProvisioner;

        let (mut host, _) = setup_host_device(
            &mut provisioner,
            1500,
            InterfaceAddress {
                ip: self.config.internal_ip,
                prefix_len: 32,
            },
        )
        .await?;

        if let Some(prefix) = self.config.ipam_ipv4_subnet {
            let route = Route {
                nexthop: Some(self.config.internal_ip),
                local: Some(self.config.internal_ip),
                prefix,
                mtu: Some(1500),
                ..Default::default()
            };
            host.add_route(&route).await?;
        }

        loader_handle
            .add_endpoint(EndpointKind::Host, SARENA_HOST)
            .await?;

        set_endpoint_config(host.mac(), self.config.internal_ip);

        let ipam = Arc::new(sarena_services_ipam::DefaultIpamService::new(
            self.config.gateway_ip,
            self.config.ipam_ipv4_subnet,
            self.config.ipam_ipv6_subnet,
        ));
        let endpoint = Arc::new(sarena_services_endpoint::DefaultEndpointService::new(
            loader_handle,
            provisioner,
            PIN_ROOT,
        ));
        let daemon = Arc::new(sarena_services_daemon::DefaultDaemonService::new());

        let state = AppState::new(ipam, endpoint, daemon);

        Ok(state)
    }
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
