use core::net::Ipv4Addr;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
    pub ipv4: Ipv4Addr,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointInfo {
    // The HOST-side veth ifindex, valid in the node's default namespace.
    pub if_index: u32,
    // The CONTAINER-side MAC, i.e. the endpoint's own interface address as seen
    // from inside its netns.
    pub container_mac: [u8; 6],
    pub host_mac: [u8; 6],
}

#[cfg(feature = "std")]
mod pod_impls {
    use super::{EndpointConfig, EndpointInfo};

    unsafe impl aya::Pod for EndpointInfo {}
    unsafe impl aya::Pod for EndpointConfig {}
}

#[cfg(feature = "std")]
mod display_impls {
    use core::fmt;

    use super::EndpointInfo;

    impl fmt::Display for EndpointInfo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "if_index={} container_mac=", self.if_index)?;
            write_mac(f, self.container_mac)?;
            write!(f, " host_mac=")?;
            write_mac(f, self.host_mac)
        }
    }

    fn write_mac(f: &mut fmt::Formatter<'_>, mac: [u8; 6]) -> fmt::Result {
        for (i, byte) in mac.iter().enumerate() {
            if i != 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
