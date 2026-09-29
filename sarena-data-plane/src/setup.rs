use std::{net::IpAddr, sync::Arc, thread::JoinHandle};

use async_trait::async_trait;
use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    route::Route,
};
use sarena_shared::EndpointConfig;

use crate::{
    AyaBackend, DataPlane, DataPlaneError, EndpointConfigMap, EndpointKind, Loader, LoaderHandle,
    PIN_ROOT, PinRoot,
    config::DataPlaneConfig,
    error::Res,
    netlink::{SARENA_HOST, setup_host_device},
};

#[derive(Clone)]
pub struct DefaultDataPlane {
    inner: Arc<DefaultDataPlaneInner>,
}

#[async_trait]
impl DataPlane for DefaultDataPlane {
    type Error = DataPlaneError;

    async fn shutdown(self) -> Res<()> {
        let inner = Arc::into_inner(self.inner).ok_or(DataPlaneError::StillShared)?;
        inner.loader_handle.shutdown(inner.loader_thread).await
    }

    fn loader_handle(&self) -> LoaderHandle {
        self.inner.loader_handle.clone()
    }
}

impl DefaultDataPlane {
    pub async fn new(config: &DataPlaneConfig) -> Res<Self> {
        let data_plane = start_control_plane(config).await?;
        Ok(Self {
            inner: Arc::new(data_plane),
        })
    }
}

pub struct DefaultDataPlaneInner {
    loader_handle: LoaderHandle,
    loader_thread: JoinHandle<()>,
}

async fn start_control_plane(config: &DataPlaneConfig) -> Res<DefaultDataPlaneInner> {
    let pin_root = PinRoot::new(PIN_ROOT);

    std::fs::create_dir_all(pin_root.globals_dir()).expect("creating globals dir");

    let backend = AyaBackend::new(pin_root.clone());
    let loader: Loader<AyaBackend> = Loader::new(backend, pin_root.clone());

    let (loader_handle, loader_thread) = LoaderHandle::spawn(loader, 16);

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

    Ok(DefaultDataPlaneInner {
        loader_handle,
        loader_thread,
    })
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
