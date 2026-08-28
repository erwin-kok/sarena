use std::net::IpAddr;

use ipnet::IpNet;

pub struct ControlPlaneConfig {
    pub gateway_ip: IpAddr,
    pub internal_ip: IpAddr,
    pub ipam_ipv4_subnet: Option<IpNet>,
    pub ipam_ipv6_subnet: Option<IpNet>,
}
