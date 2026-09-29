use std::path::PathBuf;

use sarena_infra::InfraError;
use thiserror::Error;

#[derive(Debug)]
pub struct HookFailure {
    pub hook_name: &'static str,
    pub error: String,
}

#[derive(Debug, Error)]
pub enum DataPlaneError {
    #[error("failed to look up link {name:?}: {src}")]
    LinkLookup { name: String, src: String },

    #[error("could not create veth pair {0}")]
    CouldNotCreateVethPair(String),

    #[error("Prefix length error: {0}")]
    PrefixLengthError(#[from] ipnet::PrefixLenError),

    #[error("infra error: {0}")]
    InfraError(#[from] InfraError),

    #[error("failed to load eBPF object: {0}")]
    ObjectLoad(String),

    #[error("program {name:?} not found in the loaded object")]
    ProgramNotFound { name: &'static str },

    #[error("failed to load program {name:?}: {src}")]
    ProgramLoad { name: &'static str, src: String },

    #[error("failed to attach program {name:?} to {ifname:?}: {src}")]
    Attach {
        name: &'static str,
        ifname: String,
        src: String,
    },

    #[error("failed to unpin {path:?}: {src}")]
    Unpin { path: PathBuf, src: String },

    #[error("failed to resolve link {link:?}: {src}")]
    LinkResolve { link: String, src: String },

    #[error("failed to list pins under {prefix:?}: {src}")]
    ListPins { prefix: PathBuf, src: String },

    #[error("map {name:?} not found in the loaded object")]
    MapNotFound { name: String },

    #[error("failed to open pinned map at {path:?}: {src}")]
    MapOpen { path: PathBuf, src: String },

    #[error("operation on map {map:?} failed: {src}")]
    MapAccess { map: &'static str, src: String },

    #[error("{} required hook(s) failed to attach", .0.len())]
    Partial(Vec<HookFailure>),

    #[error("loader actor is no longer running")]
    ActorGone,

    #[error("cannot shut down data plane while other clones are still alive")]
    StillShared,

    #[cfg(any(test, feature = "test-util"))]
    #[error("injected failure: {0}")]
    Injected(String),
}

pub type Res<T> = Result<T, DataPlaneError>;
