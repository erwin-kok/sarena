#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct EndpointConfig {
    pub mac: [u8; 6],
}
