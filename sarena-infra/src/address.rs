use std::{fmt, net::IpAddr, str::FromStr};

use ipnet::IpNet;

use crate::InfraError;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AddressFamily {
    V4,
    V6,
}

impl AddressFamily {
    pub const fn matches(self, ip: &IpAddr) -> bool {
        match (self, ip) {
            (Self::V4, IpAddr::V4(_)) | (Self::V6, IpAddr::V6(_)) => true,
            (Self::V4, IpAddr::V6(_)) | (Self::V6, IpAddr::V4(_)) => false,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct InterfaceAddress {
    pub ip: IpAddr,
    pub prefix_len: u8,
}

impl InterfaceAddress {
    pub const fn new(ip: IpAddr, prefix_len: u8) -> Result<Self, InfraError> {
        let max = max_prefix_len(ip);

        if prefix_len > max {
            return Err(InfraError::PrefixLenError(prefix_len, max));
        }

        Ok(Self { ip, prefix_len })
    }

    pub const fn to_ip_net(self) -> IpNet {
        IpNet::new_assert(self.ip, self.prefix_len)
    }
}

impl From<InterfaceAddress> for IpNet {
    fn from(addr: InterfaceAddress) -> Self {
        addr.to_ip_net()
    }
}

impl From<IpNet> for InterfaceAddress {
    fn from(net: IpNet) -> Self {
        Self {
            ip: net.addr(),
            prefix_len: net.prefix_len(),
        }
    }
}

const fn max_prefix_len(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

impl FromStr for InterfaceAddress {
    type Err = InfraError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(prefix) = s.parse::<IpNet>() {
            return Self::new(prefix.addr(), prefix.prefix_len());
        }

        let addr = s
            .parse::<IpAddr>()
            .map_err(|e| InfraError::InterfaceAddressParseError(e.to_string()))?;

        let prefix_len = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };

        Self::new(addr, prefix_len)
    }
}

impl fmt::Display for InterfaceAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.ip, self.prefix_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_ip_net_keeps_host_bits_and_prefix() {
        let addr: InterfaceAddress = "10.0.0.7/24".parse().unwrap();
        let net = addr.to_ip_net();

        assert_eq!(net, "10.0.0.7/24".parse::<IpNet>().unwrap());
        assert_eq!(net.addr(), addr.ip);
        assert_eq!(net.prefix_len(), 24);
        // `From` delegates to the same conversion.
        assert_eq!(IpNet::from(addr), net);
    }

    #[test]
    fn to_ip_net_defaults_to_host_prefix() {
        let addr: InterfaceAddress = "2001:db8::1".parse().unwrap();
        assert_eq!(addr.to_ip_net().prefix_len(), 128);
    }
}
