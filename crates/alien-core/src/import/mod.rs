//! Typed setup import contract.
//!
//! `alien-core` owns the request shape every CloudFormation / Terraform /
//! Helm generator and every importer (manager-side or agent-side) speaks:
//!
//! * [`data`] — typed `Aws*ImportData` / `Gcp*ImportData` / `Azure*ImportData`
//!   structs. Pure data + JsonSchema, camelCase serde. The payload that
//!   crosses crate boundaries.
//! * [`request`] — [`StackImportRequest`] / [`ImportedResource`] /
//!   [`StackImportResponse`] / [`ImportSourceKind`]. The HTTP request /
//!   response types of `POST /v1/stack/import`.
//! * [`context`] — [`EmitContext`] / [`ImportContext`]. Pure data passed
//!   through to format emitters and to importers respectively.
//!
//! Format-specific traits, registries, and emitters live in their respective
//! format crates:
//!
//! * `alien_cloudformation::CfEmitter` + `CfRegistry` (also owns
//!   `CfResource` / `CfExpression`).
//! * `alien_terraform::TfEmitter` + `TfRegistry` (returns `hcl::Block` /
//!   `hcl::Expression` directly — no intermediate IR).
//! * `alien_helm::HelmEmitter` + `HelmRegistry`.
//!
//! Importers live next to their controllers in `alien-infra`:
//!
//! * `alien_infra::ResourceImporter` + `ImporterRegistry`.
//!
//! This split keeps `alien-core` lightweight (no format dependencies) and
//! lets each format crate use its native types without an intermediate IR.

pub mod context;
pub mod data;
pub mod request;

pub use context::*;
pub use data::*;
pub use request::*;
