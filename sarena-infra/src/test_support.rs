use std::{
    future::Future,
    path::PathBuf,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Netns, NetnsGuard};

pub fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .subsec_nanos();
    // Linux interface names are capped at IFNAMSIZ-1 (15) bytes, and this
    // helper is also used to name veth ends directly, so keep it short.
    format!("{prefix}{:x}", (process::id() ^ nanos) & 0xffff)
}

#[allow(dead_code)]
pub async fn with_temp_netns<F, Fut, T>(prefix: &str, body: F) -> T
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: Future<Output = T>,
{
    let name = unique_name(prefix);
    Netns::create(&name)
        .await
        .expect("failed to create temporary test namespace");

    let _guard = NetnsGuard::new(name.clone());

    body(Netns::path_for(&name)).await
}
