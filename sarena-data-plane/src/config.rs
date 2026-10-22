use std::net::IpAddr;

use ipnet::IpNet;

use crate::PinRoot;

pub struct DataPlaneConfig {
    pub gateway_ip: IpAddr,
    pub internal_ip: IpAddr,
    pub ipam_ipv4_subnet: Option<IpNet>,
    pub ipam_ipv6_subnet: Option<IpNet>,
    pub pin_root: PinRoot,
}
