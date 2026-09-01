use crate::ResourceLifecycle;

/// When a resource type contributes resources to the setup artifact.
///
/// Most types answer with the lifecycle alone. A sandbox does not: a Live one still needs the
/// setup stack to create its build role, because the runtime controller may only *pass* that role
/// — `sandbox/provision` grants `iam:PassRole` and no `iam:CreateRole`, the same shape
/// `worker/provision` uses for the service-account role a Worker is passed. Only the image itself
/// moves to runtime, and the emitter decides that from the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupEmission {
    /// Never part of the setup artifact; a runtime controller owns the whole resource.
    Never,
    /// Emitted only when the resource is Frozen.
    WhenFrozen,
    /// Emitted under either lifecycle, with the emitter narrowing what it writes.
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

    pub const fn should_emit_in_setup(self, lifecycle: ResourceLifecycle) -> bool {
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
/// granted by a repository policy naming the customer's account, which is not known until the
/// deployment registers, and registration cannot precede a setup-stack build. Frozen stays the
/// default so a stack that already installed a setup-stack image keeps it.
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
        // Both, and that is the point: a Live sandbox still needs the setup stack to create the
        // build role its runtime controller is only permitted to pass. The emitter narrows what
        // it writes; the policy does not.
        assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        assert!(policy.should_emit_in_setup(ResourceLifecycle::Live));
        assert!(policy.requires_management_permissions());
        assert!(policy.has_runtime_cleanup_before_teardown());
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
