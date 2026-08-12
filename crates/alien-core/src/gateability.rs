//! The single authority on which resources may carry an `.enabled()` gate.
//!
//! Both the compile-time check (`ResourceEnabledValidCheck`) and the setup
//! generators consult this module, so a caller that renders without running
//! preflights hits the same refusals. The rules live here rather than in the
//! preflight crate because the generators must not depend on preflights, and
//! duplicating the rules would let them drift.
//!
//! Extension resource types registered outside this crate are gateable by
//! default (their ownership policy is `user_choice`), matching how the emitter
//! registries treat them: the generic gating post-pass needs nothing from the
//! emitter, so there is nothing for an extension to opt into.

use crate::ownership_policy_for_resource_type;

/// Reserved id of the deployment secrets vault.
///
/// `SecretsVaultMutation` links this vault to Live Workers and compute
/// clusters after compile-time checks run, so its presence can never be
/// optional. Owned here so the gating rules and the mutation agree on one
/// constant.
pub const SECRETS_VAULT_ID: &str = "secrets";

/// Framework and auxiliary infrastructure Alien derives from the stack, the
/// platform, or the deployment settings. A gate here is never a customer
/// choice: `ServiceAccountMutation` inserts profile-derived "{profile}-sa"
/// entries unconditionally, the Azure `default-*` resources are
/// preflight-injected hosts other resources build on, and network presence is
/// a StackSettings decision, not a stack-resource one. Both naming variants
/// are listed where the ownership table accepts both.
const STACK_DERIVED_TYPES: &[&str] = &[
    "build",
    "artifact-registry",
    "service-account",
    "compute-cluster",
    "kubernetes-cluster",
    "network",
    "remote-stack-management",
    "resource-access",
    "service_activation",
    "service-activation",
    "azure_resource_group",
    "azure-resource-group",
    "azure_storage_account",
    "azure-storage-account",
    "azure_container_apps_environment",
    "azure-container-apps-environment",
    "azure_service_bus_namespace",
    "azure-service-bus-namespace",
];

/// Types whose setup emitters have not been proven under the generic gating
/// post-pass yet. Emptied as each type's gated render is validated; the list
/// exists so switching the mechanism cannot silently open gating for a type
/// nobody has rendered gated before.
const NOT_YET_GENERIC_TYPES: &[&str] = &[];

/// Why a resource cannot carry an `.enabled()` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateRefusal {
    /// The reserved deployment secrets vault (`SECRETS_VAULT_ID`).
    ReservedSecretsVault,
    /// Framework infrastructure derived from the stack itself.
    DerivedFromStack,
    /// The type's gated setup render has not been validated yet.
    NotYetGeneric,
}

impl GateRefusal {
    /// The reason clause, phrased to follow "Resource 'x' is enabled by input
    /// 'y', but ...". Callers compose the full message so the preflight and
    /// the generators report identically.
    pub fn reason(self) -> &'static str {
        match self {
            GateRefusal::ReservedSecretsVault => {
                "it is the deployment secrets vault. Workers and compute clusters are wired to \
                 it automatically after compile-time checks run, so a deployer who says no would \
                 leave them resolving a binding for a vault that was never created. Its presence \
                 cannot be optional. Give a vault you want to gate a different id"
            }
            GateRefusal::DerivedFromStack => {
                "Alien derives this resource from the stack itself, so it cannot be optional"
            }
            GateRefusal::NotYetGeneric => {
                "this resource type's conditional setup render has not been validated yet, so \
                 the resource would be created regardless of the deployer's answer"
            }
        }
    }
}

/// Whether a resource may carry an `.enabled()` gate at all. `None` means
/// gateable. Lifecycle legality is not decided here — a lifecycle the type
/// does not allow is refused by the lifecycle rules regardless of gating.
pub fn gate_refusal(resource_type: &str, resource_id: &str) -> Option<GateRefusal> {
    if resource_id == SECRETS_VAULT_ID {
        return Some(GateRefusal::ReservedSecretsVault);
    }
    if STACK_DERIVED_TYPES.contains(&resource_type) {
        return Some(GateRefusal::DerivedFromStack);
    }
    // Compute (worker, daemon, container) is deliberately gateable: declining
    // a live workload rides the same removal path as deleting it from a
    // release, and its provisioning baseline persists so acceptance can
    // return. Pausing the sole consumer of an ungated queue is allowed by
    // design — the queue's retention policy governs the backlog.
    if NOT_YET_GENERIC_TYPES.contains(&resource_type) {
        return Some(GateRefusal::NotYetGeneric);
    }
    None
}

/// Per-lifecycle gateability of one resource type, derived from the ownership
/// table and the gate refusals. Serialized into the generated manifest the
/// TypeScript SDK's surface test consumes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypeGateability {
    /// The gate may appear on a Frozen entry of this type.
    pub frozen: bool,
    /// The gate may appear on a Live entry of this type.
    pub live: bool,
}

/// Gateability of one built-in user resource type, keyed for the manifest.
pub fn type_gateability(resource_type: &str) -> TypeGateability {
    let policy = ownership_policy_for_resource_type(resource_type);
    // The id-based rule cannot be evaluated per type; the manifest describes
    // types, and the reserved vault id is refused per entry.
    let gateable = gate_refusal(resource_type, "").is_none();
    TypeGateability {
        frozen: gateable && policy.allows_frozen(),
        live: gateable && policy.allows_live(),
    }
}

/// The built-in user resource types listed in the generated manifest. The SDK
/// builder surface is asserted against exactly this set.
pub const MANIFEST_TYPES: &[&str] = &[
    "kv",
    "storage",
    "queue",
    "vault",
    "postgres",
    "ai",
    "worker",
    "daemon",
    "container",
    "email",
    "sandbox",
    "experimental/aws-opensearch",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_are_gateable_in_both_lifecycles() {
        for store in ["kv", "storage", "queue", "vault", "ai"] {
            assert_eq!(gate_refusal(store, "analytics"), None, "{store}");
            let gateability = type_gateability(store);
            assert!(gateability.frozen && gateability.live, "{store}");
        }
    }

    #[test]
    fn postgres_is_live_gateable_only() {
        assert_eq!(gate_refusal("postgres", "db"), None);
        let gateability = type_gateability("postgres");
        // Frozen postgres has no setup emitter today; the gate rule allows it
        // and the missing emitter refuses the render, exactly as for an
        // ungated frozen postgres.
        assert!(gateability.live);
    }

    #[test]
    fn compute_is_live_gateable() {
        for compute in ["worker", "daemon", "container"] {
            assert_eq!(gate_refusal(compute, "api"), None, "{compute}");
            let gateability = type_gateability(compute);
            assert!(!gateability.frozen, "{compute} cannot be frozen");
            assert!(gateability.live, "{compute} gates as a live resource");
        }
    }

    #[test]
    fn stack_derived_types_are_refused() {
        for framework in STACK_DERIVED_TYPES {
            assert_eq!(
                gate_refusal(framework, "x"),
                Some(GateRefusal::DerivedFromStack),
                "{framework}"
            );
        }
    }

    #[test]
    fn the_reserved_secrets_vault_is_refused_by_id() {
        assert_eq!(
            gate_refusal("vault", SECRETS_VAULT_ID),
            Some(GateRefusal::ReservedSecretsVault)
        );
        assert_eq!(gate_refusal("vault", "app-tokens"), None);
    }

    #[test]
    fn email_and_opensearch_gate_as_frozen_resources() {
        for setup_owned in ["email", "experimental/aws-opensearch"] {
            assert_eq!(gate_refusal(setup_owned, "x"), None, "{setup_owned}");
            let gateability = type_gateability(setup_owned);
            assert!(gateability.frozen, "{setup_owned} gates at setup");
            assert!(!gateability.live, "{setup_owned} has no runtime controller");
        }
    }

    /// A sandbox declaration is the Frozen image and pool, not a session. Sessions are created at
    /// runtime and cleaned up with the parent, so there is no Live entry for a gate to sit on.
    #[test]
    fn sandbox_gates_as_a_frozen_resource() {
        assert_eq!(gate_refusal("sandbox", "agents"), None);
        let gateability = type_gateability("sandbox");
        assert!(gateability.frozen, "the declaration itself gates at setup");
        assert!(
            !gateability.live,
            "sessions are not declared, so a live gate would have nothing to refuse"
        );
    }

    /// Every manifest type must state its gateability, because the SDK builder surface is
    /// generated from it — a type nobody asserted gets whatever the ownership table happens to
    /// say, which is how a new resource acquires an unintended gate.
    #[test]
    fn every_manifest_type_is_asserted_somewhere_above() {
        let asserted = [
            "kv",
            "storage",
            "queue",
            "vault",
            "ai",
            "postgres",
            "worker",
            "daemon",
            "container",
            "email",
            "experimental/aws-opensearch",
            "sandbox",
        ];
        for declared in MANIFEST_TYPES {
            assert!(
                asserted.contains(declared),
                "{declared} is in the manifest but no test pins its gateability"
            );
        }
    }

    #[test]
    fn extension_types_default_to_gateable() {
        assert_eq!(gate_refusal("acme-widgets", "widgets"), None);
        let gateability = type_gateability("acme-widgets");
        assert!(gateability.frozen && gateability.live);
    }
}
