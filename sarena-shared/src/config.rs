use core::net::Ipv4Addr;

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
    pub ipv4: Ipv4Addr,
}

#[cfg(feature = "std")]
unsafe impl aya::Pod for EndpointConfig {}
