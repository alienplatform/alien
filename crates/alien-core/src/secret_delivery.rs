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
    /// "secrets" vault through the Worker runtime wrapper. Worker-only:
    /// runtime-less workloads have nothing that could consume the pointer.
    VaultPointer,
    /// The hosting layer delivers each applicable secret natively before the
    /// process starts — Kubernetes `valueFrom.secretKeyRef`, the local
    /// supervisor's resolved plain env, or Horizon workload secrets — so no
    /// vault-load pointer is ever injected.
    NativeProjection,
}

impl SecretDelivery {
    /// Resolves how a compute kind's secrets are delivered.
    ///
    /// - Kubernetes and Machines hosting layers project Worker secrets before
    ///   process start. Other Worker hosts use the runtime vault pointer.
    /// - Containers and Daemons are runtime-less on EVERY platform
    ///   (ALIEN-211): nothing in the workload can load a vault pointer, so
    ///   the hosting layer projects secrets natively before process start
    ///   (Kubernetes secretKeyRef, local supervisor plain env, Horizon
    ///   workload secrets) and the pointer must never be minted for them.
    ///
    /// The match is exhaustive over both enums so a new platform or compute
    /// kind forces an explicit delivery choice here.
    pub fn resolve(platform: Platform, kind: ComputeKind) -> Self {
        match (platform, kind) {
            (_, ComputeKind::Container | ComputeKind::Daemon) => SecretDelivery::NativeProjection,
            (Platform::Kubernetes | Platform::Machines, ComputeKind::Worker) => {
                SecretDelivery::NativeProjection
            }
            (
                Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Local | Platform::Test,
                ComputeKind::Worker,
            ) => SecretDelivery::VaultPointer,
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

    /// A runtime-less Container/Daemon has nothing that could consume a vault
    /// pointer on any platform.
    #[test]
    fn containers_and_daemons_always_project_natively() {
        for kind in [ComputeKind::Container, ComputeKind::Daemon] {
            assert_eq!(
                SecretDelivery::resolve(Platform::Aws, kind),
                SecretDelivery::NativeProjection,
                "{} must never receive the vault pointer",
                kind.as_str()
            );
        }
    }

    #[test]
    fn worker_delivery_depends_on_host_capability() {
        for platform in [Platform::Kubernetes, Platform::Machines] {
            assert_eq!(
                SecretDelivery::resolve(platform, ComputeKind::Worker),
                SecretDelivery::NativeProjection,
                "{platform:?} projects Worker secrets"
            );
        }
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Local,
            Platform::Test,
        ] {
            assert_eq!(
                SecretDelivery::resolve(platform, ComputeKind::Worker),
                SecretDelivery::VaultPointer,
                "{platform:?} Worker uses the runtime vault pointer"
            );
        }
    }
}
