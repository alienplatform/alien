//! How a compute resource's Secret-typed env vars reach the running workload.
//!
//! The deployment layer injects Secret-typed env vars one of two ways, and the
//! choice depends on the platform and the compute kind. Modeling both as typed
//! enums keeps the decision in one exhaustive `match` — a new [`Platform`] or
//! [`ComputeKind`] variant fails to compile until it picks a delivery — instead
//! of stringly-typed `matches!(resource_type, "container" | "daemon")` checks
//! scattered across crates.

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
    /// - Workers ship the runtime wrapper on every platform and load their
    ///   secrets from the vault via the pointer.
    /// - Containers and Daemons are runtime-less on EVERY platform
    ///   (ALIEN-211): nothing in the workload can load a vault pointer, so
    ///   the hosting layer projects secrets natively before process start
    ///   (Kubernetes secretKeyRef, local supervisor plain env, Horizon
    ///   workload secrets) and the pointer must never be minted for them.
    ///
    /// The match is exhaustive over `ComputeKind` so a new compute kind
    /// forces an explicit delivery choice here.
    pub fn resolve(kind: ComputeKind) -> Self {
        match kind {
            ComputeKind::Worker => SecretDelivery::VaultPointer,
            ComputeKind::Container | ComputeKind::Daemon => SecretDelivery::NativeProjection,
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

    /// The ALIEN-211 delete-list invariant: the ALIEN_SECRETS pointer is
    /// Worker-only. A runtime-less Container/Daemon has nothing that could
    /// consume it, on any platform.
    #[test]
    fn containers_and_daemons_always_project_natively() {
        for kind in [ComputeKind::Container, ComputeKind::Daemon] {
            assert_eq!(
                SecretDelivery::resolve(kind),
                SecretDelivery::NativeProjection,
                "{} must never receive the vault pointer",
                kind.as_str()
            );
        }
    }

    #[test]
    fn workers_use_the_vault_pointer() {
        assert_eq!(
            SecretDelivery::resolve(ComputeKind::Worker),
            SecretDelivery::VaultPointer
        );
    }
}
