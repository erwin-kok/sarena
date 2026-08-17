use core::net::Ipv4Addr;

// This is config is a private struct per endpoint.
// There is should be only one config per endpoint and gives the configuration of that particular
// endpoint.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
    pub ipv4: Ipv4Addr,
}

// This info is a struct intended for public information.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointInfo {
    pub if_index: u32,
}

#[cfg(feature = "std")]
mod pod_impls {
    use super::{EndpointConfig, EndpointInfo};

    unsafe impl aya::Pod for EndpointInfo {}
    unsafe impl aya::Pod for EndpointConfig {}
}
