//! Alien SDK for Rust.
//!
//! Cloud-agnostic bindings for storage, KV, queues, vaults, and linked containers, plus the
//! Worker application context.
//! Works on AWS, GCP, Azure, Kubernetes, and locally.
//!
//! This is the public-facing crate for Alien app developers. Its binding API is
//! deliberately limited to [`Bindings`] and the kinds applications use directly:
//! [`Storage`], [`Kv`], [`Queue`], [`Vault`], and [`Container`].
//!
//! Platform tooling that needs provider construction or managed resource kinds
//! such as builds and artifact registries uses `alien_bindings` directly.
//! Worker applications enable the `worker` Cargo feature and use the
//! `worker` module for task, event, lifecycle, and `waitUntil` APIs. The
//! feature is not enabled by default, so direct-binding-only applications do
//! not depend on the Worker protocol.
//!
//! # Example
//!
//! ```no_run
//! use alien_sdk::Bindings;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let bindings = Bindings::from_env()?;
//!     let cache = bindings.kv("cache").await?;
//!     cache.put("greeting", b"hello".to_vec(), None).await?;
//!     assert_eq!(cache.get("greeting").await?, Some(b"hello".to_vec()));
//!     Ok(())
//! }
//! ```
//!
//! Provider construction and managed resource bindings are not part of this
//! crate's application API:
//!
//! ```compile_fail
//! use alien_sdk::BindingsProvider;
//! ```
//!
//! ```compile_fail
//! use alien_sdk::BindingsProviderApi;
//! ```
//!
//! ```compile_fail
//! use alien_sdk::Binding;
//! ```
//!
//! ```compile_fail
//! use alien_sdk::{ArtifactRegistry, Build, Postgres, ServiceAccount, Worker};
//! ```
//!
//! ```compile_fail
//! use alien_sdk::provider;
//! ```
//!
//! ```compile_fail
//! use alien_sdk::providers;
//! ```
//!
//! ```compile_fail
//! use alien_sdk::http_client;
//! ```
//!
pub use alien_bindings::{
    Bindings, BoundQueue as Queue, Container, ErrorData, Kv, Result, Storage, Vault,
};

/// Errors returned by the application binding and Worker APIs.
pub mod error {
    pub use alien_bindings::error::{Error, ErrorData, Result};
}

/// Serializable requests returned by storage presigning operations.
pub mod presigned {
    pub use alien_bindings::presigned::{
        LocalOperation, PresignedOperation, PresignedRequest, PresignedRequestBackend,
        PresignedResponse,
    };
}

/// App-facing binding value types (the option/message/result types that flow
/// through storage/KV/queue/vault/container calls).
pub mod traits {
    pub use alien_bindings::traits::{
        Kv, MessagePayload, PutOptions, QueueMessage, ScanResult, Storage, Vault,
    };
    pub use alien_bindings::{BoundQueue as Queue, Container};
}

#[cfg(feature = "worker")]
mod alien_context;
#[cfg(feature = "worker")]
mod wait_until;

/// Worker-only task, event, lifecycle, and `waitUntil` APIs.
#[cfg(feature = "worker")]
pub mod worker {
    //! Worker task, event, lifecycle, and `waitUntil` APIs.
    //!
    //! `AlienContext` exposes the same application [`Bindings`]
    //! facade, not the internal provider API:
    //!
    //! ```compile_fail
    //! fn load_internal_binding(ctx: &alien_sdk::worker::AlienContext) {
    //!     let _ = ctx.bindings().load_build("builder");
    //! }
    //! ```
    //!
    //! ```compile_fail
    //! fn inspect_managed_resource(ctx: &alien_sdk::worker::AlienContext) {
    //!     let _ = ctx.get_current_worker();
    //!     let _ = ctx.get_current_container();
    //! }
    //! ```

    pub use crate::alien_context::{AlienContext, CronEvent, QueueMessage, StorageEvent};
    pub use crate::wait_until::{DrainConfig, DrainResponse, WaitUntil, WaitUntilContext};
}
