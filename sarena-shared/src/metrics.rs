pub const OBS_POINT_CONTAINER_FORWARD: u8 = 0;
pub const OBS_POINT_HOST_FORWARD: u8 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MetricsKey {
    pub obs_point: u8,
    /// place for future fields
    pub pad: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MetricsValue {
    pub packets: u64,
    pub bytes: u64,
}

#[cfg(feature = "std")]
mod pod_impls {
    use super::{MetricsKey, MetricsValue};

    unsafe impl aya::Pod for MetricsKey {}
    unsafe impl aya::Pod for MetricsValue {}
}

#[cfg(feature = "std")]
pub use display_impls::write_obs_point;

#[cfg(feature = "std")]
mod display_impls {
    use super::{OBS_POINT_CONTAINER_FORWARD, OBS_POINT_HOST_FORWARD};

    pub fn write_obs_point(obs_point: u8) -> &'static str {
        match obs_point {
            OBS_POINT_CONTAINER_FORWARD => "CONT FWD",
            OBS_POINT_HOST_FORWARD => "HOST FWD",
            _ => "UNKNOWN",
        }
    }
}
