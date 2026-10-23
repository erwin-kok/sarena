use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use async_trait::async_trait;
use ipnet::IpNet;
use sarena_api_types_v1::endpoint::{
    Addressing, EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse,
    EndpointHealthStatus,
};
use sarena_data_plane::{EndpointManager, EndpointPersist, EndpointSpec, MacAddress};
use tracing::info;

use crate::{
    EndpointError,
    endpoint::{EndpointService, Res},
};

pub struct DefaultEndpointService {
    endpoint_manager: EndpointManager,
}

impl DefaultEndpointService {
    pub fn new(endpoint_manager: EndpointManager) -> Self {
        Self { endpoint_manager }
    }
}

#[async_trait]
impl EndpointService for DefaultEndpointService {
    async fn create(
        &self,
        attachment_id: String,
        request: EndpointCreateRequest,
    ) -> Res<EndpointCreateResponse> {
        info!("create endpoint {attachment_id}: {:?}", request);

        if self
            .endpoint_manager
            .lookup_container_id(&request.container_id)
            .is_some()
        {
            return Err(EndpointError::GenericError(format!(
                "endpoint for container {} already exists",
                request.container_id,
            )));
        }

        let ipv4 = parse_ipv4(request.ipv4.as_ref())?;
        let ipv6 = parse_ipv6(request.ipv6.as_ref())?;

        if let Some(ip) = ipv4
            && self.endpoint_manager.lookup_ipv4(ip).is_some()
        {
            return Err(EndpointError::GenericError(format!(
                "Ipv4 {ip} is already in use",
            )));
        }

        if let Some(ip) = ipv6
            && self.endpoint_manager.lookup_ipv6(ip).is_some()
        {
            return Err(EndpointError::GenericError(format!(
                "Ipv6 {ip} is already in use",
            )));
        }

        let host_mac = MacAddress::parse(&request.host_mac).expect("parse host mac");
        let container_mac = MacAddress::parse(&request.container_mac).expect("parse container mac");
        let lease = self.endpoint_manager.allocate_new_id()?;
        let endpoint = EndpointPersist {
            id: lease.id(),
            spec: EndpointSpec {
                container_name: None,
                container_id: Some(request.container_id),
                docker_network_id: None,
                docker_endpoint: None,
                host_iface_name: request.host_iface_name,
                host_iface_index: request.host_iface_index,
                container_mac,
                host_mac,
                ipv4,
                ipv6,
                k8s_ns: Some(request.k8s_namespace),
                k8s_pod_name: Some(request.k8s_pod_name),
            },
        };

        let ep = self.endpoint_manager.add_endpoint(&endpoint)?;
        let id = lease.commit();
        info!("endpoint {attachment_id} created with id {id}");

        ep.regenerate().await?;

        Ok(EndpointCreateResponse {})
    }

    async fn delete(&self, attachment_id: String) -> Res<()> {
        info!("delete endpoint {attachment_id}");
        Ok(())
    }

    async fn health(&self, attachment_id: String) -> Res<EndpointHealthResponse> {
        info!("endpoint health {attachment_id}");

        Ok(EndpointHealthResponse {
            heatlh: EndpointHealthStatus::Ok,
        })
    }
}

fn parse_ipv4(addr: Option<&Addressing>) -> Res<Option<Ipv4Addr>> {
    let Some(addr) = addr else {
        return Ok(None);
    };
    let ip = &addr.ip;
    let ip = ip
        .parse::<IpNet>()
        .map(|net| net.addr())
        .or_else(|_| ip.parse::<IpAddr>())
        .map_err(|e| EndpointError::GenericError(format!("error parsing ip address {ip}: {e}")))?;
    match ip {
        IpAddr::V4(ipv4) => Ok(Some(ipv4)),
        IpAddr::V6(_) => Err(EndpointError::GenericError(
            "error parsing ipv4 address".to_string(),
        )),
    }
}

fn parse_ipv6(addr: Option<&Addressing>) -> Res<Option<Ipv6Addr>> {
    let Some(addr) = addr else {
        return Ok(None);
    };
    let ip = &addr.ip;
    let ip = ip
        .parse::<IpNet>()
        .map(|net| net.addr())
        .or_else(|_| ip.parse::<IpAddr>())
        .map_err(|e| EndpointError::GenericError(format!("error parsing ip address {ip}: {e}")))?;
    match ip {
        IpAddr::V4(_) => Err(EndpointError::GenericError(
            "error parsing ipv6 address".to_string(),
        )),
        IpAddr::V6(ipv6) => Ok(Some(ipv6)),
    }
}
