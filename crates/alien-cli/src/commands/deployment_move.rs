use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::{types::MoveDeploymentRequest, SdkResultExt as _};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;

pub struct MoveOptions<'a> {
    pub destination: &'a str,
    pub dry_run: bool,
    pub expected_revision: Option<u64>,
    pub json: bool,
}

#[cfg(all(test, feature = "platform"))]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    #[tokio::test]
    async fn move_command_obeys_preview_blockers_and_revision_over_http() {
        let id = "dep_aaaaaaaaaaaaaaaaaaaaaaaa";
        let destination = "dg_bbbbbbbbbbbbbbbbbbbbbbbb";
        for (dry_run, blocked, explicit_revision) in [
            (true, false, None),
            (false, true, None),
            (false, false, None),
            (false, false, Some(5)),
        ] {
            let server = MockServer::start_async().await;
            let read = server.mock_async(|when, then| {
                when.method(GET).path(format!("/v1/deployments/{id}"));
                then.status(200).json_body(json!({
                    "id": id, "name": "sample", "status": "running", "platform": "machines",
                    "projectId": "prj_aaaaaaaaaaaaaaaaaaaaaaaa", "deploymentProtocolVersion": 1,
                    "deploymentGroupId": "dg_aaaaaaaaaaaaaaaaaaaaaaaa", "purpose": "application",
                    "stackSettings": { "deploymentModel": "push", "heartbeats": "on", "telemetry": "off",
                        "updates": "auto", "network": null, "domains": null },
                    "releaseChannel": "production", "retryRequested": false,
                    "createdAt": "2026-10-08T00:00:00Z", "updatedAt": "2026-10-08T00:00:00Z",
                    "managerId": "mgr_aaaaaaaaaaaaaaaaaaaaaaaa", "workspaceId": "ws_aaaaaaaaaaaaaaaaaaaaaaaa"
                }));
            }).await;
            let preview = server.mock_async(|when, then| {
                when.method(POST).path(format!("/v1/deployments/{id}/move"))
                    .json_body_partial(json!({"deploymentGroupId": destination, "dryRun": true}));
                then.status(200).json_body(json!({
                    "deploymentId": id, "previousDeploymentGroupId": "dg_aaaaaaaaaaaaaaaaaaaaaaaa",
                    "deploymentGroupId": destination, "membershipRevision": 17,
                    "result": "preview", "blockers": if blocked { vec!["active maintenance lease"] } else { vec![] },
                    "projectionStatus": "confirmed"
                }));
            }).await;
            let commit = server
                .mock_async(|when, then| {
                    when.method(POST)
                        .path(format!("/v1/deployments/{id}/move"))
                        .json_body_partial(
                            json!({"deploymentGroupId": destination, "dryRun": false,
                        "expectedMembershipRevision": explicit_revision.unwrap_or(17)}),
                        );
                    then.status(200).json_body(json!({
                    "deploymentId": id, "previousDeploymentGroupId": "dg_aaaaaaaaaaaaaaaaaaaaaaaa",
                    "deploymentGroupId": destination, "membershipRevision": 18,
                    "result": "moved", "blockers": [], "projectionStatus": "pending"
                }));
                })
                .await;
            let ctx = ExecutionMode::Platform {
                base_url: server.base_url(),
                api_key: Some("synthetic-key".to_string()),
                no_browser: true,
                workspace: Some("sample-workspace".to_string()),
                project: None,
            };
            let result = run(
                &ctx,
                id,
                MoveOptions {
                    destination,
                    dry_run,
                    expected_revision: explicit_revision,
                    json: true,
                },
            )
            .await;
            if blocked {
                assert!(result
                    .unwrap_err()
                    .message
                    .contains("active maintenance lease"));
            } else {
                result.expect("move command must complete through the real generated HTTP client");
            }
            read.assert_hits_async(1).await;
            preview.assert_hits_async(1).await;
            commit
                .assert_hits_async(usize::from(!dry_run && !blocked))
                .await;
        }
    }
}

pub async fn run(ctx: &ExecutionMode, reference: &str, options: MoveOptions<'_>) -> Result<()> {
    let MoveOptions {
        destination,
        dry_run,
        expected_revision,
        json,
    } = options;
    if !ctx.is_platform() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "command".to_string(),
            message: "Deployment group moves require a platform workspace.".to_string(),
        }));
    }
    let workspace = ctx.resolve_workspace_with_bootstrap(!json).await?;
    let client = ctx.sdk_client().await?;
    let deployment = crate::platform_deployment_resolver::resolve(
        ctx, &client, &workspace, reference, None, !json,
    )
    .await?;
    let id = String::from(deployment.id);
    let mut request = MoveDeploymentRequest {
        deployment_group_id: destination.try_into().into_alien_error().context(
            ErrorData::ValidationError {
                field: "deployment-group".to_string(),
                message: "Provide a destination group ID beginning with dg_.".to_string(),
            },
        )?,
        dry_run: true,
        expected_membership_revision: expected_revision,
    };
    let preview = client
        .move_deployment()
        .id(id.as_str())
        .workspace(workspace.as_str())
        .body(&request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "previewing deployment group reassignment".to_string(),
            url: None,
        })?
        .into_inner();
    if dry_run {
        if json {
            return print_json(&preview);
        }
        println!(
            "Move preview: {} → {}",
            preview.previous_deployment_group_id.as_str(),
            preview.deployment_group_id.as_str()
        );
        println!("Membership revision: {}", preview.membership_revision);
        for blocker in &preview.blockers {
            println!("Blocked: {blocker}");
        }
        return Ok(());
    }
    if !preview.blockers.is_empty() {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "deployment-group".to_string(),
            message: preview.blockers.join("; "),
        }));
    }
    request.dry_run = false;
    request.expected_membership_revision =
        Some(expected_revision.unwrap_or(preview.membership_revision));
    let response = client
        .move_deployment()
        .id(id.as_str())
        .workspace(workspace.as_str())
        .body(request)
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "reassigning deployment group membership".to_string(),
            url: None,
        })?
        .into_inner();
    if json {
        return print_json(&response);
    }
    println!(
        "Deployment {} is in group {} (revision {}).",
        id,
        response.deployment_group_id.as_str(),
        response.membership_revision
    );
    println!("Metadata synchronization: {:?}", response.projection_status);
    Ok(())
}
