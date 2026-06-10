use crate::ResourceLifecycle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceOwnershipPolicy {
    default_lifecycle: ResourceLifecycle,
    allow_frozen: bool,
    allow_live: bool,
    emit_in_setup: bool,
    requires_management_permissions: bool,
    runtime_cleanup_before_teardown: bool,
}

impl ResourceOwnershipPolicy {
    pub const fn new(
        default_lifecycle: ResourceLifecycle,
        allow_frozen: bool,
        allow_live: bool,
        emit_in_setup: bool,
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
        self.emit_in_setup && matches!(lifecycle, ResourceLifecycle::Frozen)
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
        "artifact-registry" => frozen_with_management(),
        "build"
        | "network"
        | "remote-stack-management"
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
        | "azure-service-bus-namespace" => frozen_only(),
        "storage" | "queue" | "kv" | "vault" => user_choice(),
        _ => user_choice(),
    }
}

const fn frozen_only() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Frozen, true, false, true, false, false)
}

const fn frozen_with_management() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Frozen, true, false, true, true, false)
}

const fn frozen_with_runtime_cleanup() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Frozen, true, false, true, true, true)
}

const fn live_only() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Live, false, true, false, false, false)
}

const fn removed_resource_type() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Live, false, false, false, false, false)
}

const fn user_choice() -> ResourceOwnershipPolicy {
    ResourceOwnershipPolicy::new(ResourceLifecycle::Frozen, true, true, true, false, false)
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
        for resource_type in ["storage", "queue", "kv", "vault"] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert_eq!(policy.default_lifecycle(), ResourceLifecycle::Frozen);
            assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
            assert!(!policy.should_emit_in_setup(ResourceLifecycle::Live));
        }
    }

    #[test]
    fn setup_resources_are_frozen_only() {
        for resource_type in [
            "build",
            "network",
            "remote-stack-management",
            "service-account",
            "service_activation",
            "azure_resource_group",
            "azure_storage_account",
            "azure_container_apps_environment",
            "azure_service_bus_namespace",
        ] {
            let policy = ownership_policy_for_resource_type(resource_type);
            assert!(policy.allows_lifecycle(ResourceLifecycle::Frozen));
            assert!(!policy.allows_lifecycle(ResourceLifecycle::Live));
            assert!(policy.should_emit_in_setup(ResourceLifecycle::Frozen));
        }
    }
}
