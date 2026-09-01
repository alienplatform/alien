use crate::ResourceLifecycle;

/// When a resource type contributes anything to the setup artifact.
///
/// Most types answer with the lifecycle alone. A sandbox does not: a Live one still needs the
/// setup stack to create its build role, since the runtime controller may only *pass* it —
/// `sandbox/provision` grants `iam:PassRole` and no `iam:CreateRole`. Only the image moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupEmission {
    /// Never part of the setup artifact; a runtime controller owns the whole resource.
    Never,
    /// Emitted only when the resource is Frozen.
    WhenFrozen,
    /// Scaffolding under either lifecycle. Setup still only *owns* the Frozen one — the Live
    /// resource itself belongs to a runtime controller, which is why the two accessors below
    /// disagree for exactly these types.
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceOwnershipPolicy {
    default_lifecycle: ResourceLifecycle,
    allow_frozen: bool,
    allow_live: bool,
    emit_in_setup: SetupEmission,
    requires_management_permissions: bool,
    runtime_cleanup_before_teardown: bool,
}

impl ResourceOwnershipPolicy {
    pub const fn new(
        default_lifecycle: ResourceLifecycle,
        allow_frozen: bool,
        allow_live: bool,
        emit_in_setup: SetupEmission,
        requires_management_permissions: bool,
        runtime_cleanup_before_teardown: bool,
    ) -> Self {
        Self {
            default_lifecycle,
            allow_frozen,
            allow_live,
            emit_in_setup,
            requires_management_permissions,
            runtime_cleanup_before_teardown,
        }
    }

    pub const fn default_lifecycle(self) -> ResourceLifecycle {
        self.default_lifecycle
    }

    pub const fn allows_frozen(self) -> bool {
        self.allow_frozen
    }

    pub const fn allows_live(self) -> bool {
        self.allow_live
    }

    pub const fn allows_lifecycle(self, lifecycle: ResourceLifecycle) -> bool {
        match lifecycle {
            ResourceLifecycle::Frozen => self.allow_frozen,
            ResourceLifecycle::Live => self.allow_live,
        }
    }

    /// Whether setup *owns* this resource: it creates it, and no runtime controller will.
    ///
    /// Gating, lifecycle checks and permission compilation ask this. A Live sandbox answers
    /// `false` here and `true` from [`Self::emits_setup_scaffolding`]; answering the two with
    /// one predicate classifies it as setup-created, which keeps its gate out of the runtime
    /// strip and builds the image for a deployer who declined it.
    pub const fn should_emit_in_setup(self, lifecycle: ResourceLifecycle) -> bool {
        !matches!(self.emit_in_setup, SetupEmission::Never)
            && matches!(lifecycle, ResourceLifecycle::Frozen)
    }

    /// Whether the setup artifact renders anything for this resource, its own scaffolding
    /// included — a Live sandbox's build role, which its controller may pass but not create.
    ///
    /// The generators and registration's expected set ask this, and must agree: registration
    /// refuses a payload naming a resource setup does not emit, and one missing a resource it
    /// does, so a disagreement fails every install after the stack has already completed.
    pub const fn emits_setup_scaffolding(self, lifecycle: ResourceLifecycle) -> bool {
        match self.emit_in_setup {
            SetupEmission::Never => false,
            SetupEmission::WhenFrozen => matches!(lifecycle, ResourceLifecycle::Frozen),
            SetupEmission::Always => true,
        }
    }

    pub const fn requires_management_permissions(self) -> bool {
        self.requires_management_permissions
    }

    pub const fn has_runtime_cleanup_before_teardown(self) -> bool {
        self.runtime_cleanup_before_teardown
    }

    pub fn allowed_lifecycles(self) -> &'static str {
        match (self.allow_frozen, self.allow_live) {
            (true, true) => "Frozen or Live",
            (true, false) => "Frozen",
            (false, true) => "Live",
            (false, false) => "no lifecycle",
        }
    }
}

pub fn ownership_policy_for_resource_type(resource_type: &str) -> ResourceOwnershipPolicy {
    match resource_type {
        "function" | "container-cluster" => removed_resource_type(),
        "worker" | "daemon" | "container" => live_only(),
        "compute-cluster" => frozen_with_runtime_cleanup(),
        "sandbox" => sandbox_lifecycle(),
        "artifact-registry" | "key" => frozen_with_management(),
        "build"
        | "network"
        | "remote-stack-management"
        | "resource-access"
        | "service-account"
        | "service_activation"
        | "service-activation"
        | "azure_resource_group"
        | "azure-resource-group"
        | "azure_storage_account"
        | "azure-storage-account"
        | "azure_container_apps_environment"
        | "azure-container-apps-environment"
        | "azure_service_bus_namespace"
        | "azure-service-bus-namespace"
        // Email holds durable routing state (domain identities, DKIM
        // verification, receipt rules) that setup owns end to end.
        | "email" => frozen_only(),
        // Durable search state, setup-owned only: there is no runtime
        // controller that could provision or replace the collection.
        "experimental/aws-opensearch" => frozen_only(),
        "storage" | "queue" | "kv" | "vault" | "postgres" | "ai" => user_choice(),
        _ => user_choice(),
    }
}

const fn frozen_only() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Frozen,
        true,
        false,
        SetupEmission::WhenFrozen,
        false,
        false,
    )
}

const fn frozen_with_management() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Frozen,
        true,
        false,
        SetupEmission::WhenFrozen,
        true,
        false,
    )
}

const fn frozen_with_runtime_cleanup() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Frozen,
        true,
        false,
        SetupEmission::WhenFrozen,
        true,
        true,
    )
}

/// A sandbox may be baked by the setup stack or provisioned by a runtime controller.
///
/// Live is what lets the base image come from Alien's private registry: the cross-account read is
/// granted by a repository policy naming the customer's account, which isn't known until the
/// deployment registers. Frozen stays the default so an already-installed stack keeps its image.
const fn sandbox_lifecycle() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Frozen,
        true,
        true,
        SetupEmission::Always,
        true,
        true,
    )
}

const fn live_only() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Live,
        false,
        true,
        SetupEmission::Never,
        false,
        false,
    )
}

const fn removed_resource_type() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Live,
        false,
        false,
        SetupEmission::Never,
        false,
        false,
    )
}

const fn user_choice() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(
        ResourceLifecycle::Frozen,
        true,
        true,
        SetupEmission::WhenFrozen,
        false,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_resources_are_live_only() {
        for resource_type in ["worker", "daemon", "container"] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Live);
            assert!(!policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(!policy.should_emit_in_setup(ResourceLifecycle::Live));
        }
    }

    #[test]
    fn compute_cluster_is_frozen_with_runtime_cleanup() {
        let policy = ownership_policy_for_resource_type("compute-cluster");
        assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
        assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
        assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
        assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        assert!(policy.requires_management_permissions());
        assert!(policy.has_runtime_cleanup_before_teardown());
    }

    #[test]
    fn sandbox_defaults_to_frozen_but_may_be_live() {
        let policy = ownership_policy_for_resource_type("sandbox");
        assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
        assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
        assert!(policy.allows_lifecycle(ResourceLifecycle::Live));
        assert!(policy.requires_management_permissions());
        assert!(policy.has_runtime_cleanup_before_teardown());
    }

    /// The sandbox is the only type whose two answers differ, and each half is load-bearing:
    /// setup must still install the build role its controller may only pass, while the image
    /// belongs to that controller and so must reach the runtime strip a decline runs through.
    #[test]
    fn a_live_sandbox_is_scaffolded_by_setup_but_not_owned_by_it() {
        let policy = ownership_policy_for_resource_type("sandbox");

        assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        assert!(policy.emits_setup_scaffolding(ResourceLifecycle::Frozen));

        assert!(!policy.should_emit_in_setup(ResourceLifecycle::Live));
        assert!(policy.emits_setup_scaffolding(ResourceLifecycle::Live));
    }

    /// Pins the non-regression claim for every other type at once: before the split one
    /// predicate served both questions, so any type whose answers now diverge has silently
    /// changed behaviour at nine call sites.
    #[test]
    fn only_the_sandbox_separates_ownership_from_scaffolding() {
        let types = crate::gateability::MANIFEST_TYPES
            .iter()
            .copied()
            .chain([
                "function",
                "container-cluster",
                "compute-cluster",
                "artifact-registry",
                "key",
                "build",
                "network",
                "remote-stack-management",
                "resource-access",
                "service-account",
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
                "an-unregistered-extension-type",
            ]);

        for resource_type in types {
            if resource_type == "sandbox" {
                continue;
            }
            let policy = ownership_policy_for_resource_type(resource_type);
            for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
                assert_eq!(
                    policy.should_emit_in_setup(lifecycle),
                    policy.emits_setup_scaffolding(lifecycle),
                    "'{resource_type}' answers the two questions differently under {lifecycle:?}"
                );
            }
        }
    }

    #[test]
    fn artifact_registry_is_frozen_with_management() {
        let policy = ownership_policy_for_resource_type("artifact-registry");
        assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
        assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
        assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
        assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        assert!(policy.requires_management_permissions());
        assert!(!policy.has_runtime_cleanup_before_teardown());
    }

    #[test]
    fn removed_resource_type_tags_are_not_normal_policy_entries() {
        for resource_type in ["function", "container-cluster"] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert!(!policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(!policy.requires_management_permissions());
            assert!(!policy.has_runtime_cleanup_before_teardown());
        }
    }

    #[test]
    fn data_resources_can_be_frozen_or_live() {
        for resource_type in ["storage", "queue", "kv", "vault", "postgres", "ai"] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
            assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
            assert!(!policy.should_emit_in_setup(ResourceLifecycle::Live));
        }
    }

    #[test]
    fn experimental_aws_opensearch_is_frozen_only() {
        let policy = ownership_policy_for_resource_type("experimental/aws-opensearch");
        assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
        assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
        assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
        assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        assert!(!policy.requires_management_permissions());
    }

    #[test]
    fn setup_resources_are_frozen_only() {
        for resource_type in [
            "build",
            "network",
            "remote-stack-management",
            "resource-access",
            "service-account",
            "service_activation",
            "azure_resource_group",
            "azure_storage_account",
            "azure_container_apps_environment",
            "azure_service_bus_namespace",
            "email",
        ] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        }
    }
}
