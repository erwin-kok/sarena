use std::{collections::HashMap, net::IpAddr};

use async_trait::async_trait;
use sarena_api_types_v1::ipam::IpamAllocateResponse;
use thiserror::Error;

mod bitmap;
mod host_scope;
mod service;

#[cfg(feature = "test-util")]
mod mock;

#[cfg(feature = "test-util")]
pub use mock::MockIpamService;
pub use service::DefaultIpamService;

pub const POOL_DEFAULT: &str = "default";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IpamError {
    #[error("IPv4 is disabled")]
    Ipv4Disabled,

    #[error("IPv6 is disabled")]
    Ipv6Disabled,

    #[error("IP {0} is excluded, owned by {1}")]
    ExcludedError(String, String),

    #[error("unable to restore IP {0} for {1:?}: pool name must be provided")]
    PoolRequiredForAllocate(String, String),

    #[error("no IPAM pool provided for IP release of {0}")]
    PoolRequiredForRelease(String),

    #[error("range is full")]
    RangeFull,

    #[error("provided IP is already allocated")]
    AlreadyAllocated,

    #[error("the provided network does not match the current range")]
    MismatchedNetwork,

    #[error("provided IP is not in the valid range. The range of valid IPs is {valid_range}")]
    NotInRange { valid_range: String },

    /// Port of `allocateNextFamily`'s `pool == ""` branch (which falls
    /// back to `determineIPAMPool`, backed by a `metadata` dependency
    /// this crate doesn't have yet) -- rather than guessing at that
    /// lookup, an empty pool here is just an error.
    #[error(
        "unable to determine IPAM pool for owner {0:?}: automatic pool determination is not implemented"
    )]
    PoolDeterminationUnavailable(String),
}

pub type Res<T> = Result<T, IpamError>;

#[async_trait]
pub trait IpamService: Send + Sync {
    async fn allocate(
        &self,
        family: Option<String>,
        owner: Option<String>,
        pool: Option<String>,
        expiration: bool,
    ) -> Res<IpamAllocateResponse>;

    async fn allocate_ip(&self, ip: IpAddr, owner: Option<String>, pool: Option<String>)
    -> Res<()>;

    async fn release(&self, ip: IpAddr, pool: Option<String>) -> Res<()>;

    async fn dump(&self) -> Res<(HashMap<String, String>, HashMap<String, String>, String)>;
}
