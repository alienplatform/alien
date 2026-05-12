//! Stack import endpoint for distribution artifacts.
//!
//! `POST /v1/stack/import` is the manager-side ingress for the distribution
//! rebuild (ALIEN-121). A CloudFormation Custom Resource, the Terraform
//! provider's `alien_deployment` resource, or the Helm bootstrap chart sends
//! a [`StackImportRequest`] containing every resource it has just provisioned
//! along with its typed `ImportData` payload. The handler:
//!
//! 1. Authenticates the caller via the inbound deployment-group token.
//! 2. Resolves the active stack from the requested release, or the latest
//!    release when the payload omits `releaseId`.
//! 3. Dispatches each `ImportedResource` through
//!    [`alien_infra::ImporterRegistry`] to produce a typed
//!    [`StackResourceState`] form.
//! 4. Persists or replaces an imported deployment's stack state. New imports
//!    start at `provisioning` so the manager can complete layer-3 work.
//!
//! Naming. The caller supplies `deploymentName` for the deployment row and
//! `stackPrefix` for physical resource names. Distribution adapters typically
//! use the same value, but the manager treats them as separate contracts. If a
//! deployment with that name already exists in the deployment group and was
//! also imported, the handler replaces its imported stack state. A collision
//! with a native deployment returns 409.

use std::collections::HashMap;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};

use alien_core::{
    import::{ImportContext, StackImportRequest, StackImportResponse},
    AwsEnvironmentInfo, AzureEnvironmentInfo, EnvironmentInfo, GcpEnvironmentInfo, Platform,
    RuntimeMetadata, Stack, StackResourceState, StackState,
};
use alien_error::AlienError;

use super::{auth, AppState};
use crate::auth::{Scope, Subject};
use crate::error::ErrorData;
use crate::ids;
use crate::traits::{CreateImportedDeploymentParams, CreateTokenParams, ReleaseRecord, TokenType};

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/stack/import", post(stack_import))
}

/// `POST /v1/stack/import` — Inbound: deployment-group bearer.
///
/// The body's `deploymentGroupToken` field is informational (mirrored back
/// for log correlation) — actual authentication is the standard `Authorization
/// Bearer` header processed by [`auth::require_auth`]. The handler tolerates
/// the body field being either the raw token or empty; it never reads
/// credentials from the body to make the secret path uniform with every
/// other endpoint.
#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/stack/import",
    tag = "stack-import",
    request_body = StackImportRequest,
    responses(
        (status = 200, description = "Imported stack updated", body = StackImportResponse),
        (status = 201, description = "Stack imported", body = StackImportResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Deployment group or release not found"),
        (status = 409, description = "Deployment name conflict"),
    )
))]
pub async fn stack_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StackImportRequest>,
) -> Response {
    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // The import endpoint is intentionally narrow: a deployment-group token is
    // the only credential class that has a meaningful "import into this group"
    // semantic. Workspace/admin tokens could be allowed here too, but every
    // existing call-site (CFN Custom Resource, TF provider, Helm bootstrap)
    // mints a DG token at distribution time, so widening would only weaken
    // the audit trail without adding new flows.
    let deployment_group_id = match &subject.scope {
        Scope::DeploymentGroup {
            deployment_group_id,
            ..
        } => deployment_group_id.clone(),
        _ => {
            return ErrorData::forbidden("Stack import requires a deployment-group-scoped token")
                .into_response();
        }
    };

    if req.resources.is_empty() {
        return ErrorData::bad_request(
            "Stack import payload must include at least one imported resource",
        )
        .into_response();
    }

    let deployment_name = req.deployment_name.trim().to_string();
    if deployment_name.is_empty() {
        return ErrorData::bad_request(
            "Stack import payload must include a non-empty deploymentName",
        )
        .into_response();
    }
    if req.stack_prefix.trim().is_empty() {
        return ErrorData::bad_request("Stack import payload must include a non-empty stackPrefix")
            .into_response();
    }

    let dg = match state
        .deployment_store
        .get_deployment_group(&subject, &deployment_group_id)
        .await
    {
        Ok(Some(dg)) => dg,
        Ok(None) => return ErrorData::not_found_group(&deployment_group_id).into_response(),
        Err(e) => return e.into_response(),
    };

    let release = match req.release_id.as_deref() {
        Some(release_id) => match state.release_store.get_release(&subject, release_id).await {
            Ok(Some(r)) => r,
            Ok(None) => return ErrorData::not_found_release(release_id).into_response(),
            Err(e) => return e.into_response(),
        },
        None => match state.release_store.get_latest_release(&subject).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                return ErrorData::bad_request(
                    "No release exists in this project — run `alien release` before \
                     importing a stack from a distribution artifact",
                )
                .into_response();
            }
            Err(e) => return e.into_response(),
        },
    };

    let stack = match resolve_stack(&release, req.platform) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let stack_state = match build_stack_state(&state, &subject, &req, stack) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let environment_info = infer_import_environment_info(&req);
    let runtime_metadata = import_runtime_metadata(stack);

    match state
        .deployment_store
        .get_deployment_by_name(&subject, &deployment_group_id, &deployment_name)
        .await
    {
        Ok(Some(existing)) => {
            if existing.import_source.is_none() {
                return AlienError::new(ErrorData::DeploymentNameConflict {
                    name: deployment_name.clone(),
                    deployment_group_id: deployment_group_id.clone(),
                })
                .into_response();
            }
            if !state.authz.can_update_deployment(&subject, &existing) {
                return ErrorData::forbidden("Cannot update imported deployment in this group")
                    .into_response();
            }
            let updated = match state
                .deployment_store
                .update_imported_stack_state(
                    &subject,
                    &existing.id,
                    stack_state.clone(),
                    environment_info.clone(),
                    runtime_metadata.clone(),
                    Some(release.id.clone()),
                )
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            return (
                StatusCode::OK,
                Json(StackImportResponse {
                    deployment_id: updated.id,
                    stack_settings: updated.stack_settings,
                    stack_state,
                }),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(e) => return e.into_response(),
    }

    let create_ctx = crate::auth::DeploymentCreateCtx {
        workspace_id: &dg.workspace_id,
        project_id: &dg.project_id,
        deployment_group_id: Some(&deployment_group_id),
    };
    if !state.authz.can_create_deployment(&subject, create_ctx) {
        return ErrorData::forbidden("Cannot import deployment into this group").into_response();
    }

    if dg.deployment_count >= dg.max_deployments {
        return AlienError::new(ErrorData::MaxDeploymentsReached {
            deployment_group_id: deployment_group_id.clone(),
            max_deployments: dg.max_deployments,
        })
        .into_response();
    }

    let (raw_token, key_prefix, key_hash) = ids::generate_token(TokenType::Deployment.prefix());

    let params = CreateImportedDeploymentParams {
        name: deployment_name,
        deployment_group_id: deployment_group_id.clone(),
        platform: req.platform,
        stack_settings: req.stack_settings.clone(),
        stack_state: stack_state.clone(),
        environment_info,
        runtime_metadata,
        status: "provisioning".to_string(),
        current_release_id: Some(release.id.clone()),
        import_source: req.source_kind,
        deployment_token: Some(raw_token.clone()),
        management_config: Some(req.management_config.clone()),
    };

    let created = match state
        .deployment_store
        .create_with_state(&subject, params)
        .await
    {
        Ok(d) => d,
        Err(e) => return e.into_response(),
    };

    if let Err(e) = state
        .token_store
        .create_token(CreateTokenParams {
            token_type: TokenType::Deployment,
            key_prefix,
            key_hash,
            deployment_group_id: Some(deployment_group_id),
            deployment_id: Some(created.id.clone()),
        })
        .await
    {
        return e.into_response();
    }

    (
        StatusCode::CREATED,
        Json(StackImportResponse {
            deployment_id: created.id,
            stack_settings: created.stack_settings,
            stack_state,
        }),
    )
        .into_response()
}

fn import_runtime_metadata(stack: &Stack) -> RuntimeMetadata {
    RuntimeMetadata {
        prepared_stack: Some(stack.clone()),
        ..RuntimeMetadata::default()
    }
}

fn infer_import_environment_info(req: &StackImportRequest) -> Option<EnvironmentInfo> {
    match req.platform {
        Platform::Aws => infer_aws_account_id(req).map(|account_id| {
            EnvironmentInfo::Aws(AwsEnvironmentInfo {
                account_id,
                region: req.region.clone(),
            })
        }),
        Platform::Gcp => infer_gcp_project_id(req).map(|project_id| {
            EnvironmentInfo::Gcp(GcpEnvironmentInfo {
                project_number: infer_gcp_project_number(req).unwrap_or_default(),
                project_id,
                region: req.region.clone(),
            })
        }),
        Platform::Azure => infer_azure_environment_info(req),
        _ => None,
    }
}

fn infer_gcp_project_id(req: &StackImportRequest) -> Option<String> {
    req.resources
        .iter()
        .find_map(|resource| string_field(&resource.import_data, "projectId"))
}

fn infer_gcp_project_number(req: &StackImportRequest) -> Option<String> {
    req.resources
        .iter()
        .find_map(|resource| string_field(&resource.import_data, "projectNumber"))
}

fn infer_azure_environment_info(req: &StackImportRequest) -> Option<EnvironmentInfo> {
    let subscription_id = req
        .resources
        .iter()
        .find_map(|resource| string_field(&resource.import_data, "subscriptionId"))?;
    let tenant_id = req
        .resources
        .iter()
        .find_map(|resource| string_field(&resource.import_data, "tenantId"))
        .unwrap_or_default();

    Some(EnvironmentInfo::Azure(AzureEnvironmentInfo {
        tenant_id,
        subscription_id,
        location: req.region.clone(),
    }))
}

fn infer_aws_account_id(req: &StackImportRequest) -> Option<String> {
    req.resources
        .iter()
        .find_map(|resource| string_field(&resource.import_data, "accountId"))
        .or_else(|| {
            req.resources
                .iter()
                .find_map(|resource| find_aws_account_id_in_value(&resource.import_data))
        })
}

fn string_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn find_aws_account_id_in_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => parse_aws_account_id_from_arn(value),
        serde_json::Value::Array(items) => items.iter().find_map(find_aws_account_id_in_value),
        serde_json::Value::Object(map) => map.values().find_map(find_aws_account_id_in_value),
        _ => None,
    }
}

fn parse_aws_account_id_from_arn(value: &str) -> Option<String> {
    let mut parts = value.split(':');
    if parts.next()? != "arn" {
        return None;
    }
    parts.next()?;
    parts.next()?;
    parts.next()?;
    let account_id = parts.next()?;
    if account_id.len() == 12 && account_id.chars().all(|ch| ch.is_ascii_digit()) {
        Some(account_id.to_string())
    } else {
        None
    }
}

/// Look up the stack for the platform being imported. Releases carry
/// per-platform stacks (the AWS+GCP+Azure rendering of a single `alien
/// release`); the importer can only land on whichever one the artifact
/// produced.
fn resolve_stack(
    release: &ReleaseRecord,
    platform: alien_core::Platform,
) -> crate::error::Result<&Stack> {
    release.stacks.get(&platform).ok_or_else(|| {
        AlienError::new(ErrorData::BadRequest {
            reason: format!(
                "Latest release '{}' does not contain a stack for platform '{}'",
                release.id, platform
            ),
        })
    })
}

/// Run every `ImportedResource` through the [`alien_infra::ImporterRegistry`]
/// and assemble the resulting `(resource_id → StackResourceState)` map into
/// a [`StackState`]. Errors here are surfaced verbatim — they are typed
/// `ImportRegistrationMissing` / `JsonDeserializationFailed` so the caller
/// gets enough context to fix the distribution artifact without log spelunking.
fn build_stack_state(
    state: &AppState,
    _subject: &Subject,
    req: &StackImportRequest,
    stack: &Stack,
) -> crate::error::Result<StackState> {
    let mut resources: HashMap<String, StackResourceState> = HashMap::new();

    for imported in &req.resources {
        let entry = stack.resources.get(&imported.id).ok_or_else(|| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Imported resource '{}' is not present in the active stack \
                     for platform '{}' — release the stack before importing it",
                    imported.id, req.platform
                ),
            })
        })?;

        // `ImporterRegistry::run` returns an `AlienError<alien_core::ErrorData>`
        // (the importer's typed error surface). The route's error type is
        // `AlienError<crate::error::ErrorData>`, which has a single
        // `BadRequest` variant for malformed request bodies — the registry
        // already produced a structured `code`/`message`, so we re-wrap it
        // by stringifying once rather than introducing a passthrough variant
        // on the manager's surface that would have to be kept in sync with
        // every importer-side error code.
        let resource_state = state
            .import_registry
            .run(
                &imported.resource_type,
                req.platform,
                imported.import_data.clone(),
                &ImportContext {
                    resource_id: &imported.id,
                    platform: req.platform,
                    region: &req.region,
                    stack_settings: &req.stack_settings,
                    management_config: &req.management_config,
                    resource: entry,
                },
            )
            .map_err(|err| {
                AlienError::new(ErrorData::BadRequest {
                    reason: format!(
                        "Failed to import resource '{}' (type='{}', platform='{}'): {}",
                        imported.id, imported.resource_type, req.platform, err.message
                    ),
                })
            })?;

        resources.insert(imported.id.clone(), resource_state);
    }

    let mut stack_state =
        StackState::with_resource_prefix(req.platform, req.stack_prefix.trim().to_string());
    stack_state.resources = resources;
    Ok(stack_state)
}
