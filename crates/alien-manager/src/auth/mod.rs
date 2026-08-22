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
pub mod command_capability;
pub mod subject;

pub use authz::{Authz, DeploymentCreateCtx};
pub use subject::{
    CommandCapability, GatewayLogSource, Role, Scope, Subject, SubjectKind, TelemetryCapability,
};
