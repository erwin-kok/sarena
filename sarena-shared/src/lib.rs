#![cfg_attr(not(feature = "std"), no_std)]
#![no_builtins]

mod endpoint;
mod metrics;

pub use endpoint::{EndpointConfig, EndpointInfo, EndpointKey};
#[cfg(feature = "std")]
pub use metrics::write_obs_point;
pub use metrics::{MetricsKey, MetricsValue, OBS_POINT_CONTAINER_FORWARD, OBS_POINT_HOST_FORWARD};
