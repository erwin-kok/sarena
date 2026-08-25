use std::{collections::HashMap, net::IpAddr};

use async_trait::async_trait;
use sarena_api_types_v1::ipam::{ContainerAddressing, HostAddressing, IpamAllocateResponse};

use crate::{IpamService, Res};

#[derive(Debug, Default, Clone, Copy)]
pub struct MockIpamService;

#[async_trait]
impl IpamService for MockIpamService {
    async fn allocate(
        &self,
        _family: Option<String>,
        _owner: Option<String>,
        _pool: Option<String>,
        _expiration: bool,
    ) -> Res<IpamAllocateResponse> {
        Ok(IpamAllocateResponse {
            host_addressing: HostAddressing {
                ipv4: Some("10.0.0.1".to_string()),
                ipv6: None,
            },
            ipv4: Some(ContainerAddressing {
                ip: "10.0.0.2".to_string(),
                pool: None,
            }),
            ipv6: None,
        })
    }

    async fn allocate_ip(
        &self,
        _ip: IpAddr,
        _owner: Option<String>,
        _pool: Option<String>,
    ) -> Res<()> {
        Ok(())
    }

    async fn release(&self, _ip: IpAddr, _pool: Option<String>) -> Res<()> {
        Ok(())
    }

    async fn dump(&self) -> Res<(HashMap<String, String>, HashMap<String, String>, String)> {
        Ok((HashMap::new(), HashMap::new(), "Not running".to_string()))
    }
}
