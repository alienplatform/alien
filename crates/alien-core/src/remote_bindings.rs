use crate::{ResourceEntry, ResourceLifecycle, ResourceType, Stack, StackState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBindingKind {
    Storage,
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

const DEFINITIONS: &[RemoteBindingDefinition] = &[RemoteBindingDefinition {
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
}];

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

pub fn remote_binding_definitions() -> &'static [RemoteBindingDefinition] {
    DEFINITIONS
}

/// True when the stack contains only externally published resources plus
/// unavoidable setup parents. Such a stack needs no broad runtime identity.
pub fn stack_is_bindings_only(stack: &Stack) -> bool {
    let has_binding = stack
        .resources
        .values()
        .any(|entry| remote_binding_for_entry(entry).is_some());
    has_binding
        && stack.resources.values().all(|entry| {
            remote_binding_for_entry(entry).is_some()
                || entry.config.resource_type() == crate::RemoteBindings::RESOURCE_TYPE
                || DEFINITIONS.iter().any(|definition| {
                    definition
                        .setup_support_resource_types
                        .contains(&entry.config.resource_type().as_ref())
                })
        })
}

/// State-side equivalent used during credential handoff, where the desired stack is not
/// available. The standalone identity's compiled grant list is authoritative for which resource
/// IDs belong to the bindings-only data plane.
pub fn stack_state_is_bindings_only(state: &StackState) -> bool {
    let Some(bindings) = state.resources.values().find_map(|resource| {
        (resource.resource_type == crate::RemoteBindings::RESOURCE_TYPE.as_ref())
            .then(|| resource.config.downcast_ref::<crate::RemoteBindings>())
            .flatten()
    }) else {
        return false;
    };
    if bindings.grants.is_empty() {
        return false;
    }

    state.resources.iter().all(|(resource_id, resource)| {
        resource.resource_type == crate::RemoteBindings::RESOURCE_TYPE.as_ref()
            || bindings
                .grants
                .iter()
                .any(|grant| grant.resource_id == *resource_id)
            || DEFINITIONS.iter().any(|definition| {
                definition
                    .setup_support_resource_types
                    .contains(&resource.resource_type.as_str())
            })
    })
}
