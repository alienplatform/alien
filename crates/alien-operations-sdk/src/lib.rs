//! Public SDK for authoring Alien operations plugins.
//!
//! An operations plugin exposes named operations (`plugin/operation`) that
//! run against a live deployment — read-only inspection, mutation, or
//! destructive admin actions gated by declared risk. A plugin author:
//!
//! 1. Registers typed handlers with [`TypedOperations`] and calls
//!    [`run_plugin`] from `main`.
//! 2. Generates [`CanonicalPluginManifest`] from the registry's
//!    [`TypedOperations::manifests`], including input/output schemas, risk,
//!    permissions, timeout, retries, verification, and sensitive-output policy.
//!
//! The manifest supplies cloud permissions, access-request prompts, MCP tool
//! schemas, and generated docs. See `alien operations init` to scaffold a plugin.
//!
//! [`TypedOperations`] implements the [`Plugin`] trait. Implement [`Plugin`]
//! yourself only to wrap a registry's [`TypedOperations::execute`] with
//! per-invocation setup, such as validating operator-supplied endpoint
//! configuration or bounding the whole invocation with a deadline.
//! `alien operations check`, `package`, and `publish` require metadata
//! generated from typed definitions.

pub mod docs;
pub mod error;
pub mod kubernetes;
pub mod manifest;
pub mod mcp;
pub mod plugin;
pub mod protocol;
pub mod typed;
pub mod verification;

pub use alien_core::permissions::{
    AwsBindingSpec, AwsPermissionEffect, AwsPlatformPermission, AzureBindingSpec,
    AzurePlatformPermission, BindingConfiguration, GcpBindingSpec, GcpCondition,
    GcpPlatformPermission, PermissionGrant, PermissionSet, PermissionSetReference,
    PlatformPermissions,
};
pub use docs::{generate_docs, generate_docs_canonical};
pub use error::{ErrorData, Result};
pub use kubernetes::{
    KubernetesOperationPermissions, KubernetesPermissionRule, KubernetesPermissions,
};
#[allow(deprecated)]
pub use manifest::{
    Arch, CanonicalOperationManifest, CanonicalPluginManifest, OperationManifest, PluginManifest,
    PluginSettingKind, PluginSettingManifest, RetryPolicy, RiskTier, SensitiveOutputPolicy,
    MAX_BUNDLE_EXECUTABLE_BYTES,
};
pub use mcp::{
    generate_mcp_tools, generate_mcp_tools_canonical, CanonicalMcpToolSchema, McpToolSchema,
};
pub use plugin::{dispatch, run_plugin, Plugin};
pub use protocol::{
    is_explicitly_retryable, retryable_error, PluginInvocation, PluginResult, PROTOCOL_VERSION,
    RETRYABLE_ERROR_DETAILS,
};
pub use typed::{
    OperationDefinition, OperationFailure, TypedOperations, TYPED_OPERATION_PARAMS_MAX_BYTES,
};
pub use verification::Verification;
