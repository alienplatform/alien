//! Unified authenticated-caller representation. The shape is fixed; what
//! differs at runtime is the values produced by the
//! [`super::super::traits::AuthValidator`] in use and the
//! [`super::authz::Authz`] impl that interprets them.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The unified authenticated principal. Every
/// [`super::super::traits::AuthValidator`] impl produces this type.
///
/// In the standalone binary, [`Self::workspace_id`] is always `"default"` and
/// [`Self::scope`] always carries `project_id = "default"` where applicable.
/// Embedders that resolve other values do so by supplying their own
/// validator.
///
/// `Debug` is implemented by hand to redact [`Self::bearer_token`]; do not
/// derive it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    pub kind: SubjectKind,
    /// Workspace the caller belongs to. `"default"` in the standalone binary.
    pub workspace_id: String,
    pub scope: Scope,
    pub role: Role,
    /// The raw bearer token the caller presented.
    ///
    /// Lifecycle: request-scoped only. Constructed by the AuthValidator from
    /// the `Authorization` header for the current request. Never persisted,
    /// never logged (must be redacted in any Debug or tracing output), never
    /// copied into a spawned task without an explicit decision recorded in
    /// code review (token passthrough to upstream APIs IS such a decision;
    /// passing it to a long-running reconcile loop is NOT).
    ///
    /// Available to validators that need to forward the caller's identity
    /// to an upstream authentication service (token passthrough). The
    /// default validator ignores it.
    pub bearer_token: String,
}

impl fmt::Debug for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subject")
            .field("kind", &self.kind)
            .field("workspace_id", &self.workspace_id)
            .field("scope", &self.scope)
            .field("role", &self.role)
            .field("bearer_token", &"[redacted]")
            .finish()
    }
}

/// Whether the caller is a human user (dashboard) or a non-human service
/// account (CLI, agent, deployment).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SubjectKind {
    User { id: String, email: String },
    ServiceAccount { id: String },
}

/// Scopes that an incoming bearer to a manager HTTP endpoint can carry.
///
/// This enum is intentionally narrow: it models incoming callers only.
/// Privilege classes that are exclusively outgoing (e.g. a manager
/// authenticating to an upstream control plane for command dispatch / sync /
/// heartbeat) are not represented here — they would never legitimately arrive
/// as an incoming bearer, and the validator that produces this `Subject`
/// rejects them on the incoming path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Scope {
    Workspace,
    Project {
        project_id: String,
    },
    DeploymentGroup {
        project_id: String,
        deployment_group_id: String,
    },
    Deployment {
        project_id: String,
        deployment_id: String,
    },
}

impl Scope {
    /// Convenience: the project this scope is bound to (workspace-scope returns
    /// `None`).
    pub fn project_id(&self) -> Option<&str> {
        match self {
            Scope::Workspace => None,
            Scope::Project { project_id }
            | Scope::DeploymentGroup { project_id, .. }
            | Scope::Deployment { project_id, .. } => Some(project_id),
        }
    }
}

impl Subject {
    /// Synthetic system subject for internal manager loops (deployment loop,
    /// sync handler, command dispatcher) that aren't tied to an incoming
    /// request. `bearer_token` is empty; stores that need a real bearer must
    /// fall back to their own auth path in this case.
    pub fn system() -> Self {
        Self {
            kind: SubjectKind::ServiceAccount {
                id: "system".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: String::new(),
        }
    }

    /// True for callers who should be treated as the OSS operator: workspace-
    /// scoped, admin role. Used by token-management endpoints that have no
    /// direct entity for `Authz::can_*` to gate on.
    pub fn is_workspace_admin(&self) -> bool {
        matches!(self.scope, Scope::Workspace) && self.role == Role::WorkspaceAdmin
    }
}

/// Roles the policy gates on. Scoped to the role granted on the token's scope
/// (a `WorkspaceMember` token under a `Project { project_id }` scope means
/// "member of this workspace, currently acting on this project").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    WorkspaceAdmin,
    WorkspaceMember,
    WorkspaceViewer,
    ProjectDeveloper,
    ProjectViewer,
    DeploymentManager,
    DeploymentViewer,
    DeploymentGroupDeployer,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(role: Role, scope: Scope) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-1".to_string(),
            },
            workspace_id: "default".to_string(),
            scope,
            role,
            bearer_token: "bearer".to_string(),
        }
    }

    #[test]
    fn scope_project_id_workspace_is_none() {
        assert!(sample(Role::WorkspaceAdmin, Scope::Workspace)
            .scope
            .project_id()
            .is_none());
    }

    #[test]
    fn scope_project_id_for_project_variants() {
        let s = sample(
            Role::ProjectDeveloper,
            Scope::Project {
                project_id: "p1".to_string(),
            },
        );
        assert_eq!(s.scope.project_id(), Some("p1"));
    }

    #[test]
    fn subject_round_trips_through_serde() {
        let s = sample(
            Role::DeploymentManager,
            Scope::Deployment {
                project_id: "p1".to_string(),
                deployment_id: "d1".to_string(),
            },
        );
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Subject = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.workspace_id, "default");
        match back.scope {
            Scope::Deployment {
                project_id,
                deployment_id,
            } => {
                assert_eq!(project_id, "p1");
                assert_eq!(deployment_id, "d1");
            }
            other => panic!("unexpected scope {:?}", other),
        }
    }
}
