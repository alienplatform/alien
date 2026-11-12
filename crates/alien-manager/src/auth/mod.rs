//! Unified authentication and authorization primitives.
//!
//! - [`Subject`] is the canonical authenticated principal. The default
//!   validator produces `workspace_id = "default"`. Embedders inject their
//!   own [`super::traits::AuthValidator`] when they need different identity
//!   resolution.
//! - [`Authz`] is the policy trait. The default impl
//!   ([`crate::providers::oss_authz::OssAuthz`]) is a permissive role × scope
//!   table; embedders inject their own when they need stricter policy.

pub mod authz;
pub mod aws_identity;
pub mod command_capability;
pub mod commands_key;
pub mod identity_exchange;
pub mod remote_binding_capability;
pub mod runtime_capability;
#[cfg(feature = "sqlite")]
pub mod standalone_identity;
pub mod subject;
pub mod verified_setup;

pub use authz::{Authz, DeploymentCreateCtx};
pub use subject::{
    CommandCapability, GatewayLogSource, RemoteBindingCapability, RemoteBindingGrant, Role, Scope,
    Subject, SubjectKind, TelemetryCapability,
};
