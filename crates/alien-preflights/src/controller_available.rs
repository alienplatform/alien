//! Every resource needs a controller, or a path to an external binding, on the
//! platform it is deployed to.
//!
//! Without this check a deployment fails during setup, when the platform has
//! nothing that can create the resource. On Kubernetes without a cloud behind
//! it Alien provisions only the resources below; the data-layer resources
//! (Storage, Queue, KV, Postgres, Redis, AI, artifact registries) come from
//! external bindings the installer provides, and the rest have no Kubernetes
//! implementation.
//!
//! This runs when a deployment starts, not when a release is made: a release
//! for Kubernetes doesn't say which cluster installs it. On a managed cluster
//! (EKS, GKE, AKS) the cloud behind it supplies more controllers, for example
//! a KMS key through Terraform, so the check only runs without a base cloud.

/// Resource types with a Kubernetes controller (`alien-infra` registry).
/// `alien-infra` has a test that keeps this list equal to the registry.
pub const KUBERNETES_CONTROLLER_TYPES: &[&str] = &[
    "build",
    "compute-cluster",
    "container",
    "daemon",
    "kubernetes-cluster",
    "sandbox",
    "service-account",
    "vault",
    "worker",
];

/// Resource types an installer can bind to existing infrastructure with an
/// [`alien_core::ExternalBinding`] (defined in `alien-core`, which the generators share).
pub use alien_core::EXTERNAL_BINDING_TYPES;
