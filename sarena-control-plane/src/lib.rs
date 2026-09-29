use sarena_data_plane::LoaderError;
use sarena_infra::InfraError;
use thiserror::Error;

mod config;
mod netlink;
mod setup;

pub use config::ControlPlaneConfig;
pub use setup::{PIN_ROOT, start_control_plane};

#[derive(Debug, Error)]
pub enum ControlPlaneError {
    #[error("failed to look up link {name:?}: {src}")]
    LinkLookup { name: String, src: String },

    #[error("could not create veth pair {0}")]
    CouldNotCreateVethPair(String),

    #[error("Prefix length error: {0}")]
    PrefixLengthError(#[from] ipnet::PrefixLenError),

    #[error("infra error: {0}")]
    InfraError(#[from] InfraError),

    #[error("loader error: {0}")]
    LoaderError(#[from] LoaderError),
}

pub type Res<T> = Result<T, ControlPlaneError>;
