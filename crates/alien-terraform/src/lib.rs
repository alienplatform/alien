//! Terraform module generation for Alien stacks.
//!
//! Owns the entire Terraform surface: the per-resource [`TfEmitter`] trait,
//! the [`TfRegistry`] that dispatches `(ResourceType, Platform)` pairs to
//! emitters, the K8s identity overlay for `Eks` / `Gke` / `Aks` targets, and
//! the top-level [`generate_terraform_module`] orchestration.
//!
//! All HCL output flows through `hcl-rs` directly \u2014 emitters return
//! `hcl::Block` / `hcl::Expression`. There is no intermediate IR.
//!
//! `alien-core` deliberately stays format-agnostic \u2014 only typed `ImportData`
//! payloads and wire types live there. Plugins extend the TF surface by
//! constructing a `TfRegistry` and calling `register(resource_type, platform, emitter)`
//! on top of `TfRegistry::built_in()`.

pub mod block;
mod built_ins;
mod emitter;
pub mod emitters;
pub mod expr;
mod generator;
mod k8s_identity;
mod naming;
mod registry;
mod target;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub use emitter::{TfEmitter, TfFragment};
pub use generator::{
    generate_terraform_module, ModuleFiles, TerraformHelmInstall, TerraformOptions,
    TerraformRegistration,
};
pub use registry::TfRegistry;
pub use target::TerraformTarget;
