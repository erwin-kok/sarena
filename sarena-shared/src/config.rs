use core::net::Ipv4Addr;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
    pub ipv4: Ipv4Addr,
}

#[cfg(feature = "std")]
mod pod_impls {
    use super::EndpointConfig;

    unsafe impl aya::Pod for EndpointConfig {}
}
