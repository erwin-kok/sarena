use std::net::IpAddr;

use crate::InfraError;

#[derive(Debug, Copy, Clone)]
pub struct InterfaceAddress {
    pub ip: IpAddr,
    pub prefix_len: u8,
}

impl InterfaceAddress {
    pub const fn new(ip: IpAddr, prefix_len: u8) -> Result<Self, InfraError> {
        let max_prefix_len = match ip {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };

        if prefix_len > max_prefix_len {
            return Err(InfraError::PrefixLenError(prefix_len, max_prefix_len));
        }

        Ok(Self { ip, prefix_len })
    }
}
