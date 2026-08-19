mod daemon;
mod endpoint;
mod ipam;

pub use self::{daemon::*, endpoint::*, ipam::*};

pub const DEFAULT_HOST: &str = "localhost";
pub const DEFAULT_BASE_PATH: &str = "/v1";
