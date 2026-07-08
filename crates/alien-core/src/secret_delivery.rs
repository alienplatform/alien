//! How a compute resource's Secret-typed env vars reach the running workload.
//!
//! The deployment layer injects Secret-typed env vars one of two ways, and the
//! choice depends on the platform and the compute kind. Modeling both as typed
//! enums keeps the decision in one exhaustive `match` — a new [`Platform`] or
//! [`ComputeKind`] variant fails to compile until it picks a delivery — instead
//! of stringly-typed `matches!(resource_type, "container" | "daemon")` checks
//! scattered across crates.

use crate::Platform;

/// A compute resource kind that receives injected environment variables.
///
/// Only the three compute kinds are represented; storage/queue/etc. resources
/// never carry an app env and are not part of secret delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeKind {
    Worker,
    Container,
    Daemon,
}

impl ComputeKind {
    /// The resource-type string this kind serializes as (matches each
    /// resource's `RESOURCE_TYPE`). Handy for logs and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            ComputeKind::Worker => "worker",
            ComputeKind::Container => "container",
            ComputeKind::Daemon => "daemon",
        }
    }
}

/// The mechanism by which Secret-typed env vars are delivered to a workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretDelivery {
    /// Inject the `ALIEN_SECRETS` vault-load pointer (the secret keys plus a
    /// values hash). At startup the workload loads the actual values from the
    /// "secrets" vault: Workers do this through their runtime wrapper on every
    /// platform, and — pending native local delivery (ALIEN-226) — off-Kubernetes
    /// Containers and Daemons still collapse to this same pointer.
    VaultPointer,
    /// The platform controller projects each applicable secret natively (e.g. a
    /// Kubernetes `valueFrom.secretKeyRef` against a per-workload Secret), so no
    /// vault-load pointer is injected into the manifest.
    NativeProjection,
}

impl SecretDelivery {
    /// Resolves how a compute kind's secrets are delivered on a platform.
    ///
    /// Behavior, preserved from the original per-crate checks:
    /// - Workers ship the runtime wrapper on every platform and always load
    ///   their secrets from the vault via the pointer.
    /// - Kubernetes Containers and Daemons run the app image directly (no
    ///   runtime wrapper), so their controllers project secrets natively.
    /// - Every other Container/Daemon (AWS, GCP, Azure, Machines, Local, Test)
    ///   still collapses to the vault pointer. Native local Container/Daemon
    ///   delivery is deferred (ALIEN-226) — see [`SecretDelivery::VaultPointer`].
    ///
    /// The match is exhaustive over `(Platform, ComputeKind)` so adding a
    /// platform or compute kind forces an explicit delivery choice here.
    pub fn resolve(platform: Platform, kind: ComputeKind) -> Self {
        use ComputeKind::{Container, Daemon, Worker};
        use Platform::{Aws, Azure, Gcp, Kubernetes, Local, Machines, Test};

        match (platform, kind) {
            (_, Worker) => SecretDelivery::VaultPointer,
            (Kubernetes, Container | Daemon) => SecretDelivery::NativeProjection,
            (Aws | Gcp | Azure | Machines | Local | Test, Container | Daemon) => {
                SecretDelivery::VaultPointer
            }
        }
    }

    /// Whether the platform projects this kind's secrets natively (no pointer).
    pub fn is_native_projection(self) -> bool {
        matches!(self, SecretDelivery::NativeProjection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kubernetes_projects_container_and_daemon_secrets_natively() {
        for kind in [ComputeKind::Container, ComputeKind::Daemon] {
            assert_eq!(
                SecretDelivery::resolve(Platform::Kubernetes, kind),
                SecretDelivery::NativeProjection,
                "{} on Kubernetes must project natively",
                kind.as_str()
            );
        }
    }

    #[test]
    fn workers_always_use_the_vault_pointer() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
            Platform::Machines,
            Platform::Local,
            Platform::Test,
        ] {
            assert_eq!(
                SecretDelivery::resolve(platform, ComputeKind::Worker),
                SecretDelivery::VaultPointer,
                "workers on {platform:?} must keep the vault pointer"
            );
        }
    }

    #[test]
    fn non_kubernetes_containers_and_daemons_collapse_to_the_vault_pointer() {
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Machines,
            Platform::Local,
            Platform::Test,
        ] {
            for kind in [ComputeKind::Container, ComputeKind::Daemon] {
                assert_eq!(
                    SecretDelivery::resolve(platform, kind),
                    SecretDelivery::VaultPointer,
                    "{} on {platform:?} still collapses to the vault pointer",
                    kind.as_str()
                );
            }
        }
    }
}
