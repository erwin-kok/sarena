use sarena_infra::InfraError;
use thiserror::Error;

mod config;
mod error;
pub mod loader;
pub mod maps;
mod models;
mod netlink;
pub mod reconciler;
mod setup;
pub mod soft_dataplane;

pub use config::ControlPlaneConfig;
pub use error::{HookFailure, LoaderError};
pub use loader::*;
pub use maps::{CallsMap, EndpointConfigMap, LxcMap};
pub use setup::{DEFAULT_PIN_ROOT, PIN_ROOT, start_control_plane};

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

pub type Res1<T> = Result<T, ControlPlaneError>;
