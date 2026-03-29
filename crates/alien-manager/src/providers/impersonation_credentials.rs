//! Impersonation-based credential resolver for cross-account deployments.
//!
//! Re-exports from `platform_api::credential_resolver` when the `platform`
//! feature is enabled. This module makes the resolver available for
//! standalone cross-account mode without pulling in the full platform API.

#[cfg(feature = "platform")]
pub use crate::providers::platform_api::credential_resolver::{
    impersonate_management_service_account, ImpersonationCredentialResolver,
};
