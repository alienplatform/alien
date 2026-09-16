//! Public SDK for authoring Alien operations plugins.
//!
//! An operations plugin exposes named operations (`plugin/operation`) that
//! run against a live deployment — read-only inspection, mutation, or
//! destructive admin actions gated by declared risk. A plugin author:
//!
//! 1. Implements [`plugin::Plugin`] and calls [`plugin::run_plugin`] from
//!    `main`.
//! 2. Declares a [`manifest::CanonicalPluginManifest`] (conventionally
//!    `metadata.json`) describing each operation's params, risk tier,
//!    required permissions, timeout, retries, verification, and
//!    sensitive-output handling.
//!
//! The runtime uses the manifest to generate cloud permissions, CLI help,
//! access-request prompts, MCP tool schemas, and docs — declare it once,
//! get all of those for free. See `alien operations init` to scaffold a new
//! plugin from a template.

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
    RetryPolicy, RiskTier, SensitiveOutputPolicy, MAX_BUNDLE_EXECUTABLE_BYTES,
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
