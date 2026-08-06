#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
}

#[cfg(feature = "std")]
unsafe impl aya::Pod for EndpointConfig {}
