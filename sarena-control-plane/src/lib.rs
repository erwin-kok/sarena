use sarena_infra::InfraError;
use sarena_loader::LoaderError;
use thiserror::Error;

mod config;
mod netlink;
mod setup;
mod state;

pub use config::ControlPlaneConfig;
pub use setup::ControlPlane;
pub use state::AppState;

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
