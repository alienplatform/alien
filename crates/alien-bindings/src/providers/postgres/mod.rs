//! Postgres binding providers.
//!
//! Postgres is connection-only: the provider resolves connection details and the
//! application connects with its own driver. There is no gRPC service (by design),
//! so the cloud providers that resolve a secret in-process land with each cloud plan.

pub mod local;
