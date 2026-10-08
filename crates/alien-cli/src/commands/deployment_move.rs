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
