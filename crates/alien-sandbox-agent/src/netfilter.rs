//! Bounded native netfilter transactions at startup or during an authenticated Run hook.

use std::{future::Future, io, time::Duration};

pub(crate) fn install<T>(rules: impl Future<Output = nlink::netlink::Result<T>>) -> io::Result<T> {
    // No worker threads are created with the bootstrap capability grant.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(apply(rules))
}

pub(crate) async fn apply<T>(
    rules: impl Future<Output = nlink::netlink::Result<T>>,
) -> io::Result<T> {
    tokio::time::timeout(Duration::from_secs(5), rules)
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)
}
