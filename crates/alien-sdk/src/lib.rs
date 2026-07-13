//! Alien SDK for Rust.
//!
//! Cloud-agnostic bindings for storage, KV, queues, vault, commands, and more.
//! Works on AWS, GCP, Azure, Kubernetes, and locally.
//!
//! This is the public-facing crate for Alien app developers. It re-exports the
//! **app-facing** surface of [`alien_bindings`] — the storage/KV/queue/vault
//! binding factory types, the [`Bindings`] entry point, and the plumbing an
//! app needs — under the `alien_sdk` import path.
//!
//! The internal binding *kinds* — `Worker`, `Container`, `Build`,
//! `ArtifactRegistry`, `ServiceAccount`, `Postgres` — and the provider-facing
//! `BindingsProviderApi` trait are deliberately **not** re-exported here. They
//! describe resources the platform manages, not surfaces an app calls, so they
//! stay out of the app namespace. An integration that genuinely needs one
//! (e.g. an operator or a BYOC tool) imports it from `alien_bindings` directly.
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

// App-facing surface of `alien_bindings`, re-exported explicitly (no glob) so
// the internal binding kinds never leak into the app namespace.
pub use alien_bindings::{
    // Platform detection + env plumbing.
    get_current_platform,
    get_platform_from_env,
    // Storage / KV / queue / vault factory types + the shared marker trait.
    Binding,
    // Bindings entry points.
    Bindings,
    BindingsProvider,
    // Errors.
    ErrorData,
    Kv,
    Platform,
    Queue,
    Result,
    Storage,
    Vault,
    ENV_ALIEN_DEPLOYMENT_TYPE,
    ENV_OPERATOR_BASE_PLATFORM,
};

// App-facing modules. `traits` is re-exported below as a curated subset; the
// upstream `alien_bindings::traits` also carries the internal kinds, so it is
// not re-exported wholesale.
pub use alien_bindings::{error, http_client, presigned, provider, providers};

/// App-facing binding value types (the option/message/result types that flow
/// through storage/KV/queue/vault calls).
///
/// This intentionally re-exports only the app-facing items from
/// [`alien_bindings::traits`] — never the internal binding kinds (`Worker`,
/// `Container`, `Build`, `ArtifactRegistry`, `ServiceAccount`, `Postgres`) or
/// the provider-facing `BindingsProviderApi` trait.
pub mod traits {
    pub use alien_bindings::traits::{
        Binding, Kv, MessagePayload, PutOptions, Queue, ScanResult, Storage, Vault,
    };
}

pub mod alien_context;
mod wait_until;

pub use alien_context::{AlienContext, CronEvent, QueueMessage, StorageEvent};
pub use wait_until::{DrainConfig, DrainResponse, WaitUntil, WaitUntilContext};
