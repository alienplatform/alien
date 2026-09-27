//! Shared authorization rules for short-lived remote-bindings capabilities.
//!
//! A capability names the kind (and optionally the resource) it was issued for;
//! the deployment's release current at use, not at issue, decides what the
//! requested resource is.

use alien_core::remote_bindings::RemoteBindingKind;

use crate::auth::{RemoteBindingGrant, Role, Scope, Subject};
use crate::traits::deployment_store::DeploymentRecord;

/// True when `grant` covers a binding of `kind`.
pub fn grant_covers(grant: RemoteBindingGrant, kind: RemoteBindingKind) -> bool {
    match kind {
        RemoteBindingKind::Storage | RemoteBindingKind::Key | RemoteBindingKind::Ai => {
            grant == RemoteBindingGrant::Data
        }
        RemoteBindingKind::Sandbox => grant == RemoteBindingGrant::Sandbox,
    }
}

/// Deployment write authority reaches data bindings only. A sandbox runs
/// arbitrary code in the deployment's cloud, so it needs a capability issued
/// for it by name.
pub fn write_authority_covers(kind: RemoteBindingKind) -> bool {
    match kind {
        RemoteBindingKind::Storage | RemoteBindingKind::Key | RemoteBindingKind::Ai => true,
        RemoteBindingKind::Sandbox => false,
    }
}

/// True when a resolver's scope names exactly this deployment, whatever kind it grants.
pub fn names_deployment(subject: &Subject, deployment: &DeploymentRecord) -> bool {
    subject.role == Role::RemoteBindingResolver
        && subject.workspace_id == deployment.workspace_id
        && match &subject.scope {
            Scope::RemoteBindings {
                project_id,
                deployment_id,
                ..
            }
            | Scope::Deployment {
                project_id,
                deployment_id,
            } => project_id == &deployment.project_id && deployment_id == &deployment.id,
            _ => false,
        }
}

/// Decide resolution for a subject that carries a remote-bindings capability.
/// `None`: not a resolver, apply the normal policy. A resolver always gets a
/// definite answer and never falls through to deployment permissions.
pub fn resolve_decision(
    subject: &Subject,
    deployment: &DeploymentRecord,
    kind: RemoteBindingKind,
    resource_id: &str,
) -> Option<bool> {
    match &subject.scope {
        Scope::RemoteBindings {
            project_id,
            deployment_id,
            capability,
        } => Some(
            subject.role == Role::RemoteBindingResolver
                && subject.workspace_id == deployment.workspace_id
                && project_id == &deployment.project_id
                && deployment_id == &deployment.id
                && grant_covers(capability.kind, kind)
                && capability
                    .resource_id
                    .as_deref()
                    .is_none_or(|scoped| scoped == resource_id),
        ),
        // A resolver token with no kind claim predates the claim and keeps every kind until
        // all issuers send one; tokens last minutes, so this arm can then drop to data only.
        Scope::Deployment {
            project_id,
            deployment_id,
        } if subject.role == Role::RemoteBindingResolver => {
            tracing::warn!(
                event = "remote_binding_capability_missing",
                deployment_id = %deployment.id,
                resource_id = %resource_id,
                "Remote bindings token names no binding kind; allowing it until issuers send one"
            );
            Some(
                subject.workspace_id == deployment.workspace_id
                    && project_id == &deployment.project_id
                    && deployment_id == &deployment.id,
            )
        }
        _ if subject.role == Role::RemoteBindingResolver => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{RemoteBindingCapability, SubjectKind};

    const ALL_KINDS: [RemoteBindingKind; 4] = [
        RemoteBindingKind::Storage,
        RemoteBindingKind::Key,
        RemoteBindingKind::Ai,
        RemoteBindingKind::Sandbox,
    ];

    fn deployment() -> DeploymentRecord {
        DeploymentRecord {
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            id: "d1".to_string(),
            workspace_id: "w1".to_string(),
            project_id: "p1".to_string(),
            name: "d1".to_string(),
            deployment_group_id: "dg1".to_string(),
            platform: alien_core::Platform::Local,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: None,
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: None,
            setup_fingerprint: None,
            setup_fingerprint_version: None,
            user_environment_variables: None,
            management_config: None,
            deployment_token: None,
            input_values: Default::default(),
            deployment_config: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn resolver(scope: Scope) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "resolver".to_string(),
            },
            workspace_id: "w1".to_string(),
            scope,
            role: Role::RemoteBindingResolver,
            bearer_token: String::new(),
        }
    }

    fn capability(kind: RemoteBindingGrant, resource_id: Option<&str>) -> Scope {
        Scope::RemoteBindings {
            project_id: "p1".to_string(),
            deployment_id: "d1".to_string(),
            capability: RemoteBindingCapability {
                kind,
                resource_id: resource_id.map(str::to_string),
            },
        }
    }

    #[test]
    fn a_data_capability_resolves_every_data_kind_and_never_a_sandbox() {
        let subject = resolver(capability(RemoteBindingGrant::Data, None));
        for kind in ALL_KINDS {
            assert_eq!(
                resolve_decision(&subject, &deployment(), kind, "r1"),
                Some(kind != RemoteBindingKind::Sandbox),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_sandbox_capability_resolves_only_its_named_sandbox() {
        let subject = resolver(capability(RemoteBindingGrant::Sandbox, Some("box")));
        assert_eq!(
            resolve_decision(&subject, &deployment(), RemoteBindingKind::Sandbox, "box"),
            Some(true)
        );
        assert_eq!(
            resolve_decision(&subject, &deployment(), RemoteBindingKind::Sandbox, "other"),
            Some(false)
        );
        for kind in [
            RemoteBindingKind::Storage,
            RemoteBindingKind::Key,
            RemoteBindingKind::Ai,
        ] {
            assert_eq!(
                resolve_decision(&subject, &deployment(), kind, "box"),
                Some(false),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_capability_is_bound_to_its_workspace_project_and_deployment() {
        let exact = resolver(capability(RemoteBindingGrant::Data, None));
        let mut other_workspace = exact.clone();
        other_workspace.workspace_id = "w2".to_string();
        let mut other_deployment = exact.clone();
        other_deployment.scope = Scope::RemoteBindings {
            project_id: "p1".to_string(),
            deployment_id: "d2".to_string(),
            capability: RemoteBindingCapability {
                kind: RemoteBindingGrant::Data,
                resource_id: None,
            },
        };
        let mut other_project = exact.clone();
        other_project.scope = Scope::RemoteBindings {
            project_id: "p2".to_string(),
            deployment_id: "d1".to_string(),
            capability: RemoteBindingCapability {
                kind: RemoteBindingGrant::Data,
                resource_id: None,
            },
        };
        let mut wrong_role = exact.clone();
        wrong_role.role = Role::DeploymentManager;

        for subject in [other_workspace, other_deployment, other_project, wrong_role] {
            assert_eq!(
                resolve_decision(&subject, &deployment(), RemoteBindingKind::Storage, "r1"),
                Some(false),
                "{subject:?}"
            );
        }
    }

    #[test]
    fn a_resolver_token_without_a_kind_keeps_every_kind_for_its_own_deployment_only() {
        let subject = resolver(Scope::Deployment {
            project_id: "p1".to_string(),
            deployment_id: "d1".to_string(),
        });
        for kind in ALL_KINDS {
            assert_eq!(
                resolve_decision(&subject, &deployment(), kind, "r1"),
                Some(true),
                "{kind:?}"
            );
        }
        let mut other = subject.clone();
        other.scope = Scope::Deployment {
            project_id: "p1".to_string(),
            deployment_id: "d2".to_string(),
        };
        assert_eq!(
            resolve_decision(&other, &deployment(), RemoteBindingKind::Sandbox, "r1"),
            Some(false)
        );
    }

    #[test]
    fn a_resolver_role_on_any_other_scope_is_denied_outright() {
        let subject = resolver(Scope::Workspace);
        assert_eq!(
            resolve_decision(&subject, &deployment(), RemoteBindingKind::Storage, "r1"),
            Some(false)
        );
    }

    #[test]
    fn other_subjects_fall_through_to_the_normal_policy() {
        let mut subject = resolver(Scope::Workspace);
        subject.role = Role::WorkspaceAdmin;
        assert_eq!(
            resolve_decision(&subject, &deployment(), RemoteBindingKind::Sandbox, "r1"),
            None
        );
    }
}
