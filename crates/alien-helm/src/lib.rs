//! Helm chart generation for Alien stacks.
//!
//! Owns the entire Helm surface: the per-resource [`HelmEmitter`] trait,
//! the [`HelmRegistry`] that dispatches `(ResourceType, Platform)` pairs
//! to emitters, the values.yaml / values.schema.json shape, and the
//! top-level [`generate_helm_chart`] orchestration.
//!
//! `Platform::Kubernetes` is the dispatch key — clouds layer on
//! per-cloud overlays for service-account identity (IRSA on EKS, Workload
//! Identity on GKE, Federated Identity on AKS) via the chart-level
//! `examples/<target>.yaml` files. Plugins extend the surface by
//! constructing a `HelmRegistry` and calling `register(...)` on top of
//! `HelmRegistry::built_in()`.

mod emitter;
mod emitters;
mod generator;
mod registry;

pub use emitter::{HelmEmitter, HelmFragment, InfrastructureValue, ServiceAccountIdentity};
pub use generator::{generate_helm_chart, HelmChart, HelmOptions};
pub use registry::HelmRegistry;
