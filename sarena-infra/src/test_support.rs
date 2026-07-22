use std::{
    future::Future,
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::Netns;

/// A short, likely-unique name for a test-owned namespace or link.
///
/// Not cryptographically unique -- just process id + a timestamp -- which
/// is enough to avoid collisions between concurrent `cargo test` runs on
/// the same machine without pulling in a `rand` dependency just for tests.
pub fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .subsec_nanos();
    // Linux interface names are capped at IFNAMSIZ-1 (15) bytes, and this
    // helper is also used to name veth ends directly, so keep it short.
    format!("{prefix}{:x}", (process::id() ^ nanos) & 0xffff)
}

/// Creates a throwaway namespace, runs `body` with its name, and deletes
/// the namespace afterwards regardless of whether `body` panicked --
/// mirroring this crate's own philosophy of best-effort cleanup even on
/// failure.
///
/// Note: if `body` panics, cleanup still runs (via a drop guard), but the
/// panic itself still propagates so the test still fails.
#[allow(dead_code)]
pub async fn with_temp_netns<F, Fut, T>(prefix: &str, body: F) -> T
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = T>,
{
    let name = unique_name(prefix);
    Netns::create(&name)
        .await
        .expect("failed to create temporary test namespace");

    let _guard = CleanupGuard(name.clone());

    body(name).await
}

struct CleanupGuard(String);

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let name = self.0.clone();
        let cleanup = Netns::delete(&name);
        match cleanup {
            Ok(_) => {}
            Err(e) => eprintln!("warning: failed to clean up test namespace {name:?}: {e}"),
        }
    }
}
