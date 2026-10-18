//! Netfilter bootstrap must finish before worker threads inherit reduced capabilities.

use std::{future::Future, io, time::Duration};

pub(crate) fn install(rules: impl Future<Output = nlink::netlink::Result<()>>) -> io::Result<()> {
    // No worker threads are created with the bootstrap capability grant.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(5), rules)
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)
        })
}
