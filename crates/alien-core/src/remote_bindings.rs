use crate::{ResourceEntry, ResourceType, Sandbox, SandboxEgress};

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

/// A grant is attached by the setup artifact, so only a resource it renders something for can
/// be published: every Frozen one, and the Live sandbox through its scaffolding.
pub fn remote_binding_for_entry(entry: &ResourceEntry) -> Option<&'static RemoteBindingDefinition> {
    let resource_type = entry.config.resource_type();
    (entry.remote_access
        && crate::ownership_policy_for_resource_type(resource_type.as_ref())
            .emits_setup_scaffolding(entry.lifecycle))
    .then(|| remote_binding_definition(&resource_type))
    .flatten()
}

/// Why a declaration's remote binding is one a deployment cannot deliver, if it cannot.
///
/// Two cases, both sandbox-only and both about a declared policy the remote grant cannot carry.
/// Egress: starting a session is additionally authorized as `lambda:PassNetworkConnector`, and the
/// remote grant passes only AWS's own connectors, so a customer-declared one is unreachable.
/// Preview ports: `CreateMicrovmAuthToken` has no port condition key, so the list bounds a caller
/// going through the provider but not a holder of the leased credentials — a bound that only looks
/// like one. Preflight refuses either; emitters and generated docs read this so nothing advertises
/// a grant that cannot be used.
pub fn remote_binding_undeliverable_reason(entry: &ResourceEntry) -> Option<&'static str> {
    remote_binding_for_entry(entry)?;
    let sandbox = entry.config.downcast_ref::<Sandbox>()?;

    if !matches!(sandbox.egress, SandboxEgress::Allow) {
        return Some(
            "a remotely published sandbox must declare egress 'allow'; a sandbox that routes its \
             traffic through an egress connector cannot be reached remotely",
        );
    }

    if !sandbox.preview_ports.is_empty() {
        return Some(
            "a remotely published sandbox must declare no previewPorts; the session token mint \
             carries no port condition, so the list bounds a caller reaching the sandbox through \
             its binding but not a holder of the remote credentials",
        );
    }

    None
}

/// Whether a declaration's remote binding is one a deployment can actually deliver.
pub fn remote_binding_is_deliverable(entry: &ResourceEntry) -> bool {
    remote_binding_undeliverable_reason(entry).is_none()
}

/// Whether a stack's remote bindings mean this global management set belongs to the caller's
/// identity rather than the deployment's.
///
/// The binding's own set always does. A sandbox binding additionally claims anything that reaches
/// a session, because the remote caller drives those; `reaches_a_session` decides that, so the
/// permission registry stays the single place the verbs are named.
pub fn remote_binding_claims_management_set<'a>(
    resources: impl IntoIterator<Item = &'a ResourceEntry>,
    permission_set_id: &str,
    reaches_a_session: impl Fn() -> bool,
) -> bool {
    resources.into_iter().any(|entry| {
        remote_binding_for_entry(entry).is_some_and(|definition| {
            permission_set_id == definition.permission_set
                || (definition.kind == RemoteBindingKind::Sandbox && reaches_a_session())
        })
    })
}

pub fn remote_binding_definitions() -> &'static [RemoteBindingDefinition] {
    DEFINITIONS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceLifecycle, Sandbox, SandboxCode, SandboxLimits, SandboxSessionPolicy};

    fn remote_sandbox(egress: SandboxEgress, preview_ports: Vec<u16>) -> ResourceEntry {
        let sandbox = Sandbox::new("agent-sbx".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu".to_string(),
            })
            .limits(SandboxLimits {
                cpu: "1".to_string(),
                memory: "2Gi".to_string(),
                disk: "20Gi".to_string(),
                max_processes: None,
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .preview_ports(preview_ports)
            .build();

        ResourceEntry {
            enabled_when: None,
            config: crate::Resource::new(sandbox),
            dependencies: Vec::new(),
            lifecycle: ResourceLifecycle::Frozen,
            remote_access: true,
        }
    }

    /// Every deployment today declares no ports; a refusal that caught them would be the worst
    /// outcome of adding one.
    #[test]
    fn a_remote_sandbox_declaring_no_ports_is_deliverable() {
        assert!(remote_binding_is_deliverable(&remote_sandbox(
            SandboxEgress::Allow,
            Vec::new()
        )));
    }

    /// The mint carries no port condition key, so the list bounds a caller reaching the sandbox
    /// through its binding and not a holder of the leased credentials.
    #[test]
    fn a_remote_sandbox_declaring_ports_is_refused() {
        let reason =
            remote_binding_undeliverable_reason(&remote_sandbox(SandboxEgress::Allow, vec![8080]))
                .expect("a declared port list is not deliverable to a remote caller");

        assert!(
            reason.contains("previewPorts"),
            "the refusal must name the field the user declared"
        );
    }

    /// The question only applies to a remote binding. A deployment's own compute reaching its own
    /// sandbox is not this problem, and refusing it would be a false positive.
    #[test]
    fn a_sandbox_with_no_remote_binding_may_declare_ports() {
        let mut entry = remote_sandbox(SandboxEgress::Allow, vec![8080]);
        entry.remote_access = false;

        assert_eq!(remote_binding_undeliverable_reason(&entry), None);
        assert!(remote_binding_is_deliverable(&entry));
    }

    /// Two undeliverable declarations, two reasons. Collapsing them would answer a port mistake
    /// with an egress instruction.
    #[test]
    fn each_undeliverable_declaration_answers_in_its_own_terms() {
        let egress =
            remote_binding_undeliverable_reason(&remote_sandbox(SandboxEgress::Deny, Vec::new()))
                .expect("a restricted egress is not deliverable");
        let ports =
            remote_binding_undeliverable_reason(&remote_sandbox(SandboxEgress::Allow, vec![8080]))
                .expect("a declared port list is not deliverable");

        assert_ne!(egress, ports, "one reason cannot stand in for the other");
        assert!(egress.contains("egress"));
    }
}
