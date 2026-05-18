//! Per-resource Terraform emitters, organized by cloud.
//!
//! Each cloud module groups its `(ResourceType, Platform)` emitters together.
//! The `built_ins` module wires them into a [`crate::TfRegistry`].
//!
//! Plugins layer on additional `(resource_type, platform)` emitters by
//! constructing a `TfRegistry` and calling `register(...)` on top of
//! [`crate::TfRegistry::built_in`].

pub mod aws;
pub mod azure;
pub mod gcp;
pub mod worker_environment;
