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
