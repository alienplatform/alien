//! Default authorization policy. One workspace (`"default"`), one project
//! (`"default"`). The matrix below is the source of truth.
//!
//! The Subject's `workspace_id` is always `"default"` here; we don't gate on
//! it. Authz boils down to role × scope.

use crate::auth::{Authz, DeploymentCreateCtx, Role, Scope, Subject, SubjectKind};
use crate::traits::deployment_store::{DeploymentGroupRecord, DeploymentRecord};
use crate::traits::release_store::ReleaseRecord;

/// OSS policy: any authenticated token can read any release / deployment-group
/// it has scope on. Mutations require a write role. Sync/telemetry endpoints
/// are restricted to the deployment itself or workspace-wide tokens.
pub struct OssAuthz;

impl OssAuthz {
    /// True if the subject has workspace-level write authority. In OSS this
    /// covers the legacy "admin" token (mapped to `WorkspaceAdmin`) and any
    /// workspace-scoped service account with a write role.
    fn is_workspace_writer(s: &Subject) -> bool {
        matches!(s.scope, Scope::Workspace)
            && matches!(s.role, Role::WorkspaceAdmin | Role::WorkspaceMember)
    }

    /// True if the subject has *any* read authority on the project. Used for
    /// "any authenticated token can read its own project's resources" calls.
    fn can_act_on_project(s: &Subject, _project_id: &str) -> bool {
        // OSS is single-project; project_id is always "default" and any
        // authenticated token is project-bound.
        matches!(s.kind, SubjectKind::ServiceAccount { .. } | SubjectKind::User { .. })
    }
}

impl Authz for OssAuthz {
    // -- Releases ----------------------------------------------------------

    fn can_create_release(&self, s: &Subject, _project_id: &str) -> bool {
        match s.role {
            Role::WorkspaceAdmin | Role::WorkspaceMember | Role::ProjectDeveloper => true,
            _ => false,
        }
    }

    fn can_read_release(&self, _s: &Subject, _release: &ReleaseRecord) -> bool {
        // OSS single-tenant: any valid token reads any release. Deployment
        // tokens included — agents need their target release to deploy.
        true
    }

    fn can_export_release(&self, s: &Subject, release: &ReleaseRecord) -> bool {
        self.can_read_release(s, release)
    }

    // -- Deployments -------------------------------------------------------

    fn can_create_deployment(&self, s: &Subject, _ctx: DeploymentCreateCtx<'_>) -> bool {
        match s.role {
            Role::WorkspaceAdmin
            | Role::WorkspaceMember
            | Role::ProjectDeveloper
            | Role::DeploymentGroupDeployer => true,
            _ => false,
        }
    }

    fn can_read_deployment(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        match &s.scope {
            Scope::Workspace | Scope::Project { .. } => true,
            Scope::DeploymentGroup {
                deployment_group_id,
                ..
            } => deployment_group_id == &deployment.deployment_group_id,
            Scope::Deployment { deployment_id, .. } => deployment_id == &deployment.id,
        }
    }

    fn can_update_deployment(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        if !self.can_read_deployment(s, deployment) {
            return false;
        }
        matches!(
            s.role,
            Role::WorkspaceAdmin
                | Role::WorkspaceMember
                | Role::ProjectDeveloper
                | Role::DeploymentGroupDeployer
                | Role::DeploymentManager
        )
    }

    fn can_delete_deployment(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        // Deletion is workspace-write only — a deployment-group token can
        // create/update its own deployments, but tearing them down is an
        // operator action.
        if !self.can_read_deployment(s, deployment) {
            return false;
        }
        matches!(
            s.role,
            Role::WorkspaceAdmin | Role::WorkspaceMember | Role::ProjectDeveloper
        )
    }

    // -- Deployment groups -------------------------------------------------

    fn can_create_deployment_group(&self, s: &Subject, _project_id: &str) -> bool {
        Self::is_workspace_writer(s)
    }

    fn can_read_deployment_group(&self, s: &Subject, dg: &DeploymentGroupRecord) -> bool {
        match &s.scope {
            Scope::Workspace | Scope::Project { .. } => true,
            Scope::DeploymentGroup {
                deployment_group_id,
                ..
            } => deployment_group_id == &dg.id,
            Scope::Deployment { .. } => false,
        }
    }

    fn can_update_deployment_group(&self, s: &Subject, _dg: &DeploymentGroupRecord) -> bool {
        Self::is_workspace_writer(s)
    }

    fn can_delete_deployment_group(&self, s: &Subject, _dg: &DeploymentGroupRecord) -> bool {
        Self::is_workspace_writer(s)
    }

    // -- Commands ----------------------------------------------------------

    fn can_dispatch_command(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        if !self.can_read_deployment(s, deployment) {
            return false;
        }
        matches!(
            s.role,
            Role::WorkspaceAdmin
                | Role::WorkspaceMember
                | Role::ProjectDeveloper
                | Role::DeploymentGroupDeployer
        )
    }

    fn can_read_command(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        self.can_read_deployment(s, deployment)
    }

    // -- Sync protocol -----------------------------------------------------

    fn can_sync_deployment(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        match &s.scope {
            Scope::Deployment { deployment_id, .. } => deployment_id == &deployment.id,
            Scope::DeploymentGroup {
                deployment_group_id,
                ..
            } => deployment_group_id == &deployment.deployment_group_id,
            Scope::Workspace => Self::is_workspace_writer(s),
            Scope::Project { .. } => true,
        }
    }

    fn can_acquire_deployments(&self, s: &Subject, deployments: &[DeploymentRecord]) -> bool {
        deployments.iter().all(|d| self.can_sync_deployment(s, d))
    }

    // -- Telemetry ingest --------------------------------------------------

    fn can_ingest_telemetry_for(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        // Only the deployment itself ingests its own telemetry.
        matches!(
            &s.scope,
            Scope::Deployment { deployment_id, .. } if deployment_id == &deployment.id
        )
    }

    // -- Registry proxy ----------------------------------------------------

    fn can_push_image(&self, s: &Subject, project_id: &str, _repo_name: &str) -> bool {
        if !Self::can_act_on_project(s, project_id) {
            return false;
        }
        matches!(
            s.role,
            Role::WorkspaceAdmin
                | Role::WorkspaceMember
                | Role::ProjectDeveloper
                | Role::DeploymentGroupDeployer
        )
    }

    fn can_act_on_deployment(&self, s: &Subject, deployment: &DeploymentRecord) -> bool {
        self.can_read_deployment(s, deployment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::SubjectKind;
    use chrono::Utc;

    fn admin() -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-admin".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Workspace,
            role: Role::WorkspaceAdmin,
            bearer_token: "bearer".to_string(),
        }
    }

    fn dg_token(dg_id: &str) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-dg".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::DeploymentGroup {
                project_id: "default".to_string(),
                deployment_group_id: dg_id.to_string(),
            },
            role: Role::DeploymentGroupDeployer,
            bearer_token: "bearer".to_string(),
        }
    }

    fn deployment_token(deployment_id: &str) -> Subject {
        Subject {
            kind: SubjectKind::ServiceAccount {
                id: "tok-d".to_string(),
            },
            workspace_id: "default".to_string(),
            scope: Scope::Deployment {
                project_id: "default".to_string(),
                deployment_id: deployment_id.to_string(),
            },
            role: Role::DeploymentManager,
            bearer_token: "bearer".to_string(),
        }
    }

    fn deployment(id: &str, dg: &str) -> DeploymentRecord {
        DeploymentRecord {
            id: id.to_string(),
            workspace_id: "default".to_string(),
            project_id: "default".to_string(),
            name: id.to_string(),
            deployment_group_id: dg.to_string(),
            platform: alien_core::Platform::Local,
            status: "pending".to_string(),
            stack_settings: alien_core::StackSettings::default(),
            stack_state: None,
            environment_info: None,
            runtime_metadata: None,
            current_release_id: None,
            desired_release_id: None,
            user_environment_variables: None,
            management_config: None,
            deployment_token: None,
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    #[test]
    fn admin_reads_any_deployment() {
        let dep = deployment("d1", "dg-a");
        assert!(OssAuthz.can_read_deployment(&admin(), &dep));
    }

    #[test]
    fn dg_token_only_reads_own_group() {
        let dep = deployment("d1", "dg-a");
        assert!(OssAuthz.can_read_deployment(&dg_token("dg-a"), &dep));
        assert!(!OssAuthz.can_read_deployment(&dg_token("dg-b"), &dep));
    }

    #[test]
    fn deployment_token_only_reads_self() {
        let dep = deployment("d1", "dg-a");
        assert!(OssAuthz.can_read_deployment(&deployment_token("d1"), &dep));
        assert!(!OssAuthz.can_read_deployment(&deployment_token("d2"), &dep));
    }

    #[test]
    fn telemetry_ingest_is_self_only() {
        let dep = deployment("d1", "dg-a");
        assert!(OssAuthz.can_ingest_telemetry_for(&deployment_token("d1"), &dep));
        assert!(!OssAuthz.can_ingest_telemetry_for(&admin(), &dep));
    }
}
