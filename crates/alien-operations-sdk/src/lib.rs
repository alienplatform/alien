//! Public SDK for authoring Alien operations plugins.
//!
//! An operations plugin exposes named operations (`plugin/operation`) that
//! run against a live deployment — read-only inspection, mutation, or
//! destructive admin actions gated by declared risk. A plugin author:
//!
//! 1. Implements [`plugin::Plugin`] and calls [`plugin::run_plugin`] from
//!    `main`.
//! 2. Declares a [`manifest::PluginManifest`] (conventionally
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
pub mod manifest;
pub mod mcp;
pub mod plugin;
pub mod protocol;
pub mod verification;

pub use docs::generate_docs;
pub use error::{ErrorData, Result};
pub use manifest::{
    Arch, OperationManifest, PluginManifest, RetryPolicy, RiskTier, SensitiveOutputPolicy,
};
pub use mcp::{generate_mcp_tools, McpToolSchema};
pub use plugin::{dispatch, run_plugin, Plugin};
pub use protocol::{PluginInvocation, PluginResult, PROTOCOL_VERSION};
pub use verification::Verification;
