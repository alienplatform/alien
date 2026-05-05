//! Terraform provider shim for deployment registration (ALIEN-92, T11).
//!
//! Exposes a single Terraform resource — `alien_deployment` — that registers a
//! resolved stack import payload with a running [Alien Manager][alien-manager]
//! over the typed `/v1/stack/import` endpoint introduced in ALIEN-83.
//!
//! ## Layering
//!
//! | Layer                   | Concern                                              |
//! | ----------------------- | ---------------------------------------------------- |
//! | [`schema`]              | Provider + resource schema (HCL attribute shapes).   |
//! | [`resource_alien_deployment`] | CRUD lifecycle implemented against the SDK.    |
//! | `main.rs`               | Binary entry point — speaks tfplugin6 to Terraform.  |
//!
//! Every CRUD operation funnels through [`alien_manager_api::Client`]; there is
//! no hand-rolled HTTP. Tests against the SDK therefore double as integration
//! tests for the wire protocol — if the OpenAPI spec drifts, this crate fails
//! to compile.
//!
//! ## Terraform protocol wiring
//!
//! [`tf_adapter`] uses the public Rust `tf-provider` crate to speak Terraform's
//! plugin protocol. It maps Terraform values into the typed
//! [`AlienDeploymentInput`] shape and delegates all manager calls to
//! [`resource_alien_deployment`].
//!
//! ## White-label distribution
//!
//! The platform's white-label distribution mechanism (the `TFPROV01`
//! magic-bytes footer that `apps/packages-builder` appends to the binary at
//! packaging time so vendors get a baked-in default `manager_url`) lives in
//! `platform/crates/alien-terraform-providerx`, not here. Keeping it out of
//! the OSS surface means anyone consuming this crate as a library doesn't
//! pick up packaging-detail concerns — see
//! `internal-docs/alien/distribution/15-platform-extension-pattern.md`.
//!
//! [alien-manager]: alien_manager_api

pub mod resource_alien_deployment;
pub mod schema;
pub mod tf_adapter;

pub use resource_alien_deployment::{
    create, delete, read, AlienDeploymentInput, AlienDeploymentState,
};
pub use schema::{provider_schema, resource_schema};
pub use tf_adapter::{serve_terraform_provider, ProviderOptions};
