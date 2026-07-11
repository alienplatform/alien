use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;
use crate::ui::{contextual_heading, dim_label};
use alien_error::Context;
use alien_manager_api::types::{ScopeInfo, WhoamiResponse};
use alien_manager_api::SdkResultExt as ManagerSdkResultExt;
use alien_platform_api::types::{Role, ServiceAccountSubject, Subject, SubjectScope, UserSubject};
use alien_platform_api::SdkResultExt;
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Show the current authenticated principal",
    long_about = "Display information about the current authenticated principal for the selected target.",
    after_help = "EXAMPLES:
    alien whoami
    alien whoami --json
    alien dev whoami"
)]
pub struct WhoamiArgs {
    /// Emit structured JSON output
    #[arg(long)]
    pub json: bool,
}

pub async fn whoami_task(args: WhoamiArgs, ctx: ExecutionMode) -> Result<()> {
    ctx.ensure_ready().await?;

    if let ExecutionMode::Dev { port } = ctx {
        return whoami_task_dev(args, port).await;
    }

    let client = ctx.sdk_client().await?;
    let mut request = client.whoami();
    // User principals are per-workspace on the platform; without the hint the
    // server can't pick a membership and rejects user tokens.
    if let Some(workspace) = ctx.configured_workspace() {
        request = request.workspace(workspace);
    }
    let response = request
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to get user information".to_string(),
            url: None,
        })?
        .into_inner();

    if args.json {
        print_json(&response)?;
    } else {
        println!("{}", render_subject(&response));
    }

    Ok(())
}

async fn whoami_task_dev(args: WhoamiArgs, port: u16) -> Result<()> {
    let base_url = format!("http://localhost:{port}");
    let client = alien_manager_api::Client::new(&base_url);

    let response = client
        .whoami()
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "Failed to get local manager identity".to_string(),
            url: Some(format!("{base_url}/v1/whoami")),
        })?
        .into_inner();

    if args.json {
        print_json(&response)?;
    } else {
        println!("{}", render_dev_whoami(&response));
    }

    Ok(())
}

fn render_subject(subject: &Subject) -> String {
    match subject {
        Subject::UserSubject(user) => render_user_subject(user),
        Subject::ServiceAccountSubject(service_account) => {
            render_service_account_subject(service_account)
        }
    }
}

fn render_user_subject(user: &UserSubject) -> String {
    let mut lines = vec![
        contextual_heading("Signed in as", &user.email, &[]),
        format!("{} {}", dim_label("Account ID"), user.id),
        format!("{} {}", dim_label("Account Type"), user.kind),
        format!("{} {}", dim_label("Role"), user.role),
        format!("{} {}", dim_label("Workspace ID"), user.workspace_id),
    ];
    if let Some(workspace_name) = &user.workspace_name {
        lines.push(format!(
            "{} {}",
            dim_label("Workspace Name"),
            workspace_name
        ));
    }
    lines.join("\n")
}

fn render_service_account_subject(service_account: &ServiceAccountSubject) -> String {
    let mut lines = vec![
        contextual_heading("Signed in as", "service account", &[]),
        format!("{} {}", dim_label("Account ID"), service_account.id),
        format!("{} {}", dim_label("Account Type"), service_account.kind),
        format!(
            "{} {}",
            dim_label("Role"),
            format_role(&service_account.role)
        ),
        format!(
            "{} {}",
            dim_label("Scope"),
            format_scope(&service_account.scope)
        ),
        format!(
            "{} {}",
            dim_label("Workspace ID"),
            service_account.workspace_id
        ),
    ];
    if let Some(workspace_name) = &service_account.workspace_name {
        lines.push(format!(
            "{} {}",
            dim_label("Workspace Name"),
            workspace_name
        ));
    }
    lines.join("\n")
}

// Role is an untagged wrapper with no generated Display of its own — each arm
// delegates to the wrapped role type's Display instead.
fn format_role(role: &Role) -> String {
    match role {
        Role::WorkspaceRole(role) => role.to_string(),
        Role::ProjectRole(role) => role.to_string(),
        Role::DeploymentRole(role) => role.to_string(),
        Role::DeploymentGroupRole(role) => role.to_string(),
        Role::ManagerRole(role) => role.to_string(),
    }
}

fn format_scope(scope: &SubjectScope) -> String {
    match scope {
        SubjectScope::Workspace => "workspace".to_string(),
        SubjectScope::Project { project_id } => format!("project ({project_id})"),
        SubjectScope::Deployment {
            deployment_id,
            project_id,
        } => format!("deployment ({deployment_id}, project {project_id})"),
        SubjectScope::DeploymentGroup {
            deployment_group_id,
            project_id,
        } => format!("deployment-group ({deployment_group_id}, project {project_id})"),
        SubjectScope::Manager { manager_id } => format!("manager ({manager_id})"),
    }
}

fn render_dev_whoami(response: &WhoamiResponse) -> String {
    vec![
        contextual_heading("Signed in as", &response.id, &[]),
        format!("{} {}", dim_label("Account Type"), response.kind),
        format!("{} {}", dim_label("Role"), response.role),
        format!(
            "{} {}",
            dim_label("Scope"),
            format_scope_info(&response.scope)
        ),
        format!("{} {}", dim_label("Workspace ID"), response.workspace_id),
        format!(
            "{} {}",
            dim_label("Workspace Name"),
            response.workspace_name
        ),
    ]
    .join("\n")
}

// scope.type_ is a plain wire String from the manager's own OpenAPI spec, not a
// compiler-checked enum like SubjectScope — the catch-all here is a deliberate
// exception to this file's otherwise-exhaustive matches, not an oversight.
fn format_scope_info(scope: &ScopeInfo) -> String {
    match scope.type_.as_str() {
        "workspace" => "workspace".to_string(),
        "project" => format!("project ({})", scope.project_id.as_deref().unwrap_or("-")),
        "deployment" => format!(
            "deployment ({}, project {})",
            scope.deployment_id.as_deref().unwrap_or("-"),
            scope.project_id.as_deref().unwrap_or("-"),
        ),
        "deployment-group" => format!(
            "deployment-group ({}, project {})",
            scope.deployment_group_id.as_deref().unwrap_or("-"),
            scope.project_id.as_deref().unwrap_or("-"),
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_platform_api::types::{
        DeploymentGroupRole, DeploymentRole, ManagerRole, ProjectRole, ServiceAccountSubjectKind,
        UserRole, UserSubjectKind, WorkspaceRole,
    };

    fn user_subject(workspace_name: Option<&str>) -> UserSubject {
        UserSubject {
            email: "user@example.com".to_string(),
            id: "usr_7k2m".to_string(),
            kind: UserSubjectKind::User,
            role: UserRole::WorkspaceAdmin,
            workspace_id: "ws_4b21".to_string(),
            workspace_name: workspace_name.map(|name| name.to_string()),
        }
    }

    fn service_account_subject(role: Role, scope: SubjectScope) -> ServiceAccountSubject {
        ServiceAccountSubject {
            id: "sa_01HXYZ".to_string(),
            kind: ServiceAccountSubjectKind::ServiceAccount,
            role,
            scope,
            workspace_id: "ws_4b21".to_string(),
            workspace_name: Some("Acme Corp".to_string()),
        }
    }

    #[test]
    fn user_subject_with_workspace_name() {
        insta::assert_snapshot!(render_user_subject(&user_subject(Some("Acme Corp"))));
    }

    #[test]
    fn user_subject_without_workspace_name() {
        insta::assert_snapshot!(render_user_subject(&user_subject(None)));
    }

    #[test]
    fn service_account_workspace_scope() {
        let subject = service_account_subject(
            Role::WorkspaceRole(WorkspaceRole::WorkspaceMember),
            SubjectScope::Workspace,
        );
        insta::assert_snapshot!(render_service_account_subject(&subject));
    }

    #[test]
    fn service_account_project_scope() {
        let subject = service_account_subject(
            Role::ProjectRole(ProjectRole::ProjectDeveloper),
            SubjectScope::Project {
                project_id: "proj_9f3a".to_string(),
            },
        );
        insta::assert_snapshot!(render_service_account_subject(&subject));
    }

    #[test]
    fn service_account_deployment_scope() {
        let subject = service_account_subject(
            Role::DeploymentRole(DeploymentRole::DeploymentManager),
            SubjectScope::Deployment {
                deployment_id: "dep_1a2b".to_string(),
                project_id: "proj_9f3a".to_string(),
            },
        );
        insta::assert_snapshot!(render_service_account_subject(&subject));
    }

    #[test]
    fn service_account_deployment_group_scope() {
        let subject = service_account_subject(
            Role::DeploymentGroupRole(DeploymentGroupRole::DeploymentGroupDeployer),
            SubjectScope::DeploymentGroup {
                deployment_group_id: "dg_3c4d".to_string(),
                project_id: "proj_9f3a".to_string(),
            },
        );
        insta::assert_snapshot!(render_service_account_subject(&subject));
    }

    #[test]
    fn service_account_manager_scope() {
        let subject = service_account_subject(
            Role::ManagerRole(ManagerRole::ManagerRuntime),
            SubjectScope::Manager {
                manager_id: "mgr_5e6f".to_string(),
            },
        );
        insta::assert_snapshot!(render_service_account_subject(&subject));
    }

    #[test]
    fn dev_whoami_response() {
        let response = WhoamiResponse {
            id: "sa_01HXYZ".to_string(),
            kind: "serviceAccount".to_string(),
            role: "workspace.member".to_string(),
            scope: ScopeInfo {
                type_: "project".to_string(),
                project_id: Some("proj_9f3a".to_string()),
                deployment_id: None,
                deployment_group_id: None,
            },
            workspace_id: "ws_4b21".to_string(),
            workspace_name: "Acme Corp".to_string(),
        };
        insta::assert_snapshot!(render_dev_whoami(&response));
    }

    #[test]
    fn dev_whoami_response_unknown_scope_type() {
        // A scope type the manager doesn't recognize today still renders
        // instead of panicking.
        let response = WhoamiResponse {
            id: "sa_01HXYZ".to_string(),
            kind: "serviceAccount".to_string(),
            role: "workspace.member".to_string(),
            scope: ScopeInfo {
                type_: "future-type".to_string(),
                project_id: None,
                deployment_id: None,
                deployment_group_id: None,
            },
            workspace_id: "ws_4b21".to_string(),
            workspace_name: "Acme Corp".to_string(),
        };
        insta::assert_snapshot!(render_dev_whoami(&response));
    }

    #[test]
    fn render_subject_has_no_debug_wrapper_leakage() {
        let subject = Subject::ServiceAccountSubject(service_account_subject(
            Role::WorkspaceRole(WorkspaceRole::WorkspaceMember),
            SubjectScope::Project {
                project_id: "proj_9f3a".to_string(),
            },
        ));

        let rendered = render_subject(&subject);

        assert!(!rendered.contains("ServiceAccountSubject("));
        assert!(!rendered.contains("WorkspaceRole("));
        assert!(!rendered.contains("Some("));
    }
}
