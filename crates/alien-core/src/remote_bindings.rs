use crate::{ResourceEntry, ResourceLifecycle, ResourceType, Sandbox, SandboxEgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBindingKind {
    Storage,
    Key,
    Ai,
    Sandbox,
}

/// One resource type's provider-neutral Remote Bindings contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteBindingDefinition {
    pub resource_type: &'static str,
    pub permission_set: &'static str,
    pub kind: RemoteBindingKind,
    pub description: &'static str,
    /// Setup-owned parent resources that this binding kind may require. They do not turn a
    /// bindings-only stack into an application stack.
    pub setup_support_resource_types: &'static [&'static str],
    /// Increment when the permission set's effective grants change. This makes direct setup
    /// updates reconcile permissions even when the application resource config is unchanged.
    pub revision: u32,
}

const DEFINITIONS: &[RemoteBindingDefinition] = &[
    RemoteBindingDefinition {
        resource_type: "storage",
        permission_set: "storage/remote-data-write",
        kind: RemoteBindingKind::Storage,
        description: "Read and write objects in this storage resource",
        setup_support_resource_types: &[
            "azure_resource_group",
            "azure_storage_account",
            "service_activation",
        ],
        revision: 1,
    },
    RemoteBindingDefinition {
        resource_type: "key",
        permission_set: "key/remote-cryptography",
        kind: RemoteBindingKind::Key,
        description: "Encrypt and decrypt small values with this key",
        setup_support_resource_types: &["azure_resource_group", "service_activation"],
        revision: 1,
    },
    RemoteBindingDefinition {
        resource_type: "ai",
        permission_set: "ai/invoke",
        kind: RemoteBindingKind::Ai,
        description: "Invoke models through this AI resource",
        setup_support_resource_types: &["azure_resource_group", "service_activation"],
        revision: 1,
    },
    RemoteBindingDefinition {
        resource_type: "sandbox",
        permission_set: "sandbox/remote-execute",
        kind: RemoteBindingKind::Sandbox,
        description:
            "Create and terminate sessions in this sandbox, and run arbitrary code inside them",
        // A sandbox's parent is the MicroVM image its own emitter builds, and an open-egress
        // sandbox attaches no VPC connector, so setup owes this binding no other resource.
        setup_support_resource_types: &[],
        revision: 1,
    },
];

pub fn remote_binding_definition(
    resource_type: &ResourceType,
) -> Option<&'static RemoteBindingDefinition> {
    DEFINITIONS
        .iter()
        .find(|definition| definition.resource_type == resource_type.as_ref())
}

pub fn remote_binding_for_entry(entry: &ResourceEntry) -> Option<&'static RemoteBindingDefinition> {
    (entry.remote_access && entry.lifecycle == ResourceLifecycle::Frozen)
        .then(|| remote_binding_definition(&entry.config.resource_type()))
        .flatten()
}

/// Whether a declaration's remote binding is one a deployment can actually deliver.
///
/// A sandbox reaches the network through an egress connector, and starting a session on one is
/// additionally authorized as `lambda:PassNetworkConnector`. The remote grant passes only AWS's
/// own connectors, so a customer-declared one is unreachable. Preflight refuses such a stack;
/// emitters and generated docs read this so nothing advertises a grant that cannot be used.
pub fn remote_binding_is_deliverable(entry: &ResourceEntry) -> bool {
    entry
        .config
        .downcast_ref::<Sandbox>()
        .is_none_or(|sandbox| matches!(sandbox.egress, SandboxEgress::Allow))
}

pub fn remote_binding_definitions() -> &'static [RemoteBindingDefinition] {
    DEFINITIONS
}
