//! Stack import endpoint for setup artifacts.
//!
//! `POST /v1/stack/import` is the manager-side ingress for setup-driver
//! import payloads. A CloudFormation custom resource, the Terraform
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
//! 4. Merges setup-owned state into the imported deployment's stack state. New
//!    imports with pending setup resources start at `initial-setup`; otherwise
//!    they start at `provisioning` so the manager can complete Live work.
//!
//! Naming. The caller supplies `deploymentName` for the deployment row and
//! `resourcePrefix` for physical resource names. Setup drivers may use the
//! same value, but the manager treats them as separate contracts. If a
//! deployment with that name already exists in the deployment group and was
//! also imported, the handler merges setup state. A collision
//! with a native deployment returns 409.

use std::collections::HashMap;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use sha2::{Digest, Sha256};

use alien_core::{
    import::{
        ImportContext, StackImportRequest, StackImportResponse,
        CURRENT_SETUP_IMPORT_FORMAT_VERSION, MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
    },
    is_valid_resource_prefix, AwsEnvironmentInfo, AzureEnvironmentInfo, DeploymentConfig,
    DeploymentStatus, EnvironmentInfo, EnvironmentVariablesSnapshot, ExternalBindings,
    GcpEnvironmentInfo, KubernetesCluster, Platform, ResourceLifecycle, ResourceStatus,
    RuntimeMetadata, SetupUpdateAuthorization, Stack, StackResourceState, StackState,
    RESOURCE_PREFIX_ERROR_MESSAGE,
};
use alien_error::AlienError;

use super::{auth, AppState};
use crate::auth::{Scope, Subject};
use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateImportedDeploymentParams, CreateTokenParams, DeploymentRecord, ReleaseRecord, TokenType,
    UpdateImportedDeploymentParams,
};

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
    Json(raw_req): Json<serde_json::Value>,
) -> Response {
    let req = match parse_stack_import_request(raw_req) {
        Ok(req) => req,
        Err(e) => return e.into_response(),
    };

    let subject = match auth::require_auth(&state, &headers).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // The import endpoint is intentionally narrow: a deployment-group token is
    // the only credential class that has a meaningful "import into this group"
    // semantic. Workspace/admin tokens could be allowed here too, but every
    // existing call-site (CloudFormation custom resource, Terraform provider,
    // Helm bootstrap) mints a DG token at setup time, so widening would only weaken
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
    let resource_prefix = req.resource_prefix.trim().to_string();
    if !is_valid_resource_prefix(&resource_prefix) {
        return ErrorData::bad_request(RESOURCE_PREFIX_ERROR_MESSAGE).into_response();
    }
    if let Err(e) = assert_supported_import_region(&state.config, &req) {
        return e.into_response();
    }
    let setup_metadata = match setup_metadata_for_persistence(&req) {
        Ok(metadata) => metadata,
        Err(error) => return error.into_response(),
    };

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
                     importing a stack from a setup artifact",
                )
                .into_response();
            }
            Err(e) => return e.into_response(),
        },
    };

    let source_stack = match resolve_stack(&release, req.platform) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    let prepared_stack = match prepare_import_stack(source_stack.clone(), &req).await {
        Ok(stack) => stack,
        Err(e) => return e.into_response(),
    };

    let mut stack_state = match build_stack_state(&state, &subject, &req, &prepared_stack) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    // A gated resource renders behind its input in the setup template, so its
    // absence from the import IS the deployer's answer. Imported deployments
    // enter the runner at InitialSetup with this prepared stack; leaving the
    // declined entry in would make the runner create the very resource the
    // deployer said no to. A live gated resource follows the request's input
    // values here for the same reason.
    let prepared_stack = match alien_deployment::strip_declined_resources(
        prepared_stack,
        &stack_state,
        &req.input_values,
    ) {
        Ok(stack) => stack,
        Err(e) => return e.into_response(),
    };
    let environment_info = infer_import_environment_info(&req);
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
            if !setup_contract_lane_matches(&existing, &req) {
                return AlienError::new(ErrorData::ImportedDeploymentConflict {
                    reason: format!(
                        "Imported deployment '{}' was installed for a different setup target or setup fingerprint version. Import it as a different deployment.",
                        existing.id
                    ),
                })
                .into_response();
            }
            match setup_registration_replay(&existing.setup_metadata, &setup_metadata) {
                SetupRegistrationReplay::Exact => {
                    let Some(stack_settings) = existing.stack_settings else {
                        return ErrorData::internal(
                            "imported deployment is missing stack_settings",
                        )
                        .into_response();
                    };
                    return (
                        StatusCode::OK,
                        Json(StackImportResponse {
                            deployment_id: existing.id,
                            deployment_token: existing.deployment_token,
                            stack_settings,
                            stack_state,
                        }),
                    )
                        .into_response();
                }
                SetupRegistrationReplay::Conflict => {
                    return AlienError::new(ErrorData::ImportedDeploymentConflict {
                        reason: format!(
                            "Setup registration operation for deployment '{}' was replayed with a different payload",
                            existing.id
                        ),
                    })
                    .into_response();
                }
                SetupRegistrationReplay::None => {}
            }
            if let Some(existing_stack_state) = existing.stack_state.as_ref() {
                stack_state = match merge_reimported_stack_state(
                    &state,
                    &req,
                    &prepared_stack,
                    existing_stack_state,
                    stack_state,
                ) {
                    Ok(state) => state,
                    Err(error) => return error.into_response(),
                };
            }
            if !can_accept_reimport(&existing, &stack_state, &release.id, &req) {
                return AlienError::new(ErrorData::ImportedDeploymentConflict {
                    reason: format!(
                        "Imported deployment '{}' is not in a re-importable state and the payload is not idempotent",
                        existing.id
                    ),
                })
                .into_response();
            }
            if !can_reconcile_after_import(&existing) {
                return AlienError::new(ErrorData::ImportedDeploymentConflict {
                    reason: format!(
                        "Imported deployment '{}' is currently reconciling; retry setup after it reaches a stable state",
                        existing.id
                    ),
                })
                .into_response();
            }
            let runtime_metadata =
                match reimport_runtime_metadata(&existing, &prepared_stack, &release.id, &req) {
                    Ok(metadata) => metadata,
                    Err(error) => return error.into_response(),
                };
            let should_reconcile = import_changes_deployment(
                &existing,
                &stack_state,
                &environment_info,
                &runtime_metadata,
                &release.id,
                &req,
            );
            let setup_metadata = merge_setup_metadata(&existing.setup_metadata, setup_metadata);
            let updated = match state
                .deployment_store
                .update_imported_stack_state(
                    &subject,
                    &existing.id,
                    UpdateImportedDeploymentParams {
                        stack_state: stack_state.clone(),
                        environment_info: environment_info.clone(),
                        runtime_metadata: runtime_metadata.clone(),
                        setup_metadata,
                        current_release_id: Some(release.id.clone()),
                        setup_target: req.setup_target.clone(),
                        setup_fingerprint: req.setup_fingerprint.clone(),
                        setup_fingerprint_version: req.setup_fingerprint_version,
                        schedule_reconciliation: should_reconcile,
                        input_values: req.input_values.clone(),
                    },
                )
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            let stack_settings = match updated.stack_settings {
                Some(settings) => settings,
                None => {
                    return ErrorData::internal("imported deployment is missing stack_settings")
                        .into_response();
                }
            };
            return (
                StatusCode::OK,
                Json(StackImportResponse {
                    deployment_id: updated.id,
                    deployment_token: updated.deployment_token,
                    stack_settings,
                    stack_state,
                }),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(e) => return e.into_response(),
    }

    let runtime_metadata = import_runtime_metadata(&prepared_stack);

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
        deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        name: deployment_name,
        deployment_group_id: deployment_group_id.clone(),
        platform: req.platform,
        base_platform: req.base_platform,
        stack_settings: req.stack_settings.clone(),
        stack_state: stack_state.clone(),
        environment_info,
        runtime_metadata,
        status: initial_import_status(&prepared_stack, &stack_state),
        current_release_id: None,
        desired_release_id: Some(release.id.clone()),
        import_source: req.source_kind,
        setup_metadata,
        setup_target: req.setup_target.clone(),
        setup_fingerprint: req.setup_fingerprint.clone(),
        setup_fingerprint_version: req.setup_fingerprint_version,
        deployment_token: Some(raw_token.clone()),
        management_config: req.management_config.clone(),
        input_values: req.input_values.clone(),
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

    let stack_settings = match created.stack_settings {
        Some(settings) => settings,
        None => {
            return ErrorData::internal("created deployment is missing stack_settings")
                .into_response();
        }
    };
    (
        StatusCode::CREATED,
        Json(StackImportResponse {
            deployment_id: created.id,
            deployment_token: Some(raw_token),
            stack_settings,
            stack_state,
        }),
    )
        .into_response()
}

fn merge_reimported_stack_state(
    state: &AppState,
    req: &StackImportRequest,
    stack: &Stack,
    existing: &StackState,
    mut imported: StackState,
) -> crate::error::Result<StackState> {
    for resource in &req.resources {
        let Some(existing_resource) = existing.resources.get(&resource.id) else {
            continue;
        };
        let imported_resource = imported.resources.remove(&resource.id).ok_or_else(|| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Imported resource '{}' is missing from generated state",
                    resource.id
                ),
            })
        })?;
        if existing_resource.resource_type != imported_resource.resource_type {
            return Err(AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Cannot re-import resource '{}': existing type '{}' does not match imported type '{}'",
                    resource.id, existing_resource.resource_type, imported_resource.resource_type
                ),
            }));
        }
        let platform = import_platform_for_resource(state, req, &resource.resource_type);
        if existing_resource
            .controller_platform
            .is_some_and(|existing_platform| existing_platform != platform)
        {
            return Err(AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Cannot re-import resource '{}': existing controller platform does not match '{}'",
                    resource.id, platform
                ),
            }));
        }
        let entry = stack.resources.get(&resource.id).ok_or_else(|| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Imported resource '{}' is absent from the prepared stack",
                    resource.id
                ),
            })
        })?;
        let merged = state
            .import_registry
            .merge_reimport(
                &resource.resource_type,
                platform,
                existing_resource.clone(),
                imported_resource,
                &ImportContext {
                    resource_id: &resource.id,
                    platform,
                    region: &req.region,
                    stack_settings: &req.stack_settings,
                    management_config: req.management_config.as_ref(),
                    resource: entry,
                },
            )
            .map_err(|error| {
                AlienError::new(ErrorData::BadRequest {
                    reason: format!(
                        "Failed to merge re-imported resource '{}': {}",
                        resource.id, error.message
                    ),
                })
            })?;
        imported.resources.insert(resource.id.clone(), merged);
    }
    Ok(imported)
}

fn assert_supported_import_region(
    config: &crate::config::ManagerConfig,
    req: &StackImportRequest,
) -> crate::error::Result<()> {
    let setup_platform = req.base_platform.unwrap_or(req.platform);
    if setup_platform != Platform::Aws || config.supported_aws_regions.is_empty() {
        return Ok(());
    }

    if config
        .supported_aws_regions
        .iter()
        .any(|supported| supported == &req.region)
    {
        return Ok(());
    }

    Err(ErrorData::bad_request(format!(
        "Unsupported AWS region '{}' for stack import. Supported regions: {}",
        req.region,
        config.supported_aws_regions.join(", ")
    )))
}

fn parse_stack_import_request(raw: serde_json::Value) -> crate::error::Result<StackImportRequest> {
    let found_version = raw
        .get("setupImportFormatVersion")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|version| *version > 0)
        .ok_or_else(|| {
            AlienError::new(ErrorData::IncompatibleSetupImport {
                found_version: 0,
                min_supported_version: MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
                current_version: CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                repair: "Use a setup package that emits setupImportFormatVersion, or upgrade the setup package."
                    .to_string(),
            })
        })?;

    if !(MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION..=CURRENT_SETUP_IMPORT_FORMAT_VERSION)
        .contains(&found_version)
    {
        return Err(AlienError::new(ErrorData::IncompatibleSetupImport {
            found_version,
            min_supported_version: MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
            current_version: CURRENT_SETUP_IMPORT_FORMAT_VERSION,
            repair: "Use a setup package compatible with this manager, or upgrade the manager."
                .to_string(),
        }));
    }

    serde_json::from_value(raw).map_err(|err| {
        AlienError::new(ErrorData::BadRequest {
            reason: format!("Invalid stack import payload: {err}"),
        })
    })
}

fn setup_contract_lane_matches(existing: &DeploymentRecord, req: &StackImportRequest) -> bool {
    existing.setup_target.as_deref() == Some(req.setup_target.as_str())
        && existing.setup_fingerprint_version == Some(req.setup_fingerprint_version)
}

const SETUP_REGISTRATION_OPERATION_ID: &str = "setupRegistrationOperationId";
const SETUP_REGISTRATION_IMPORT_HASH: &str = "setupRegistrationImportHash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupRegistrationReplay {
    Exact,
    Conflict,
    None,
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Array(values) => {
            let values = values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{values}]")
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
            let entries = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::Value::String(key.clone()),
                        canonical_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{entries}}}")
        }
        scalar => scalar.to_string(),
    }
}

fn setup_metadata_for_persistence(
    req: &StackImportRequest,
) -> Result<Option<serde_json::Value>, AlienError<ErrorData>> {
    let Some(serde_json::Value::Object(mut metadata)) = req.setup_metadata.clone() else {
        return Ok(req.setup_metadata.clone());
    };
    let Some(serde_json::Value::String(operation_id)) =
        metadata.get(SETUP_REGISTRATION_OPERATION_ID)
    else {
        return Ok(Some(serde_json::Value::Object(metadata)));
    };
    if operation_id.is_empty() {
        return Ok(Some(serde_json::Value::Object(metadata)));
    }

    metadata.remove(SETUP_REGISTRATION_IMPORT_HASH);
    let mut request = req.clone();
    request.deployment_group_token.clear();
    request.setup_metadata = Some(serde_json::Value::Object(metadata.clone()));
    let request = serde_json::to_value(request).map_err(|error| {
        AlienError::new(ErrorData::BadRequest {
            reason: format!("Failed to fingerprint stack import payload: {error}"),
        })
    })?;
    let hash = format!("{:x}", Sha256::digest(canonical_json(&request).as_bytes()));
    metadata.insert(
        SETUP_REGISTRATION_IMPORT_HASH.to_string(),
        serde_json::Value::String(hash),
    );
    Ok(Some(serde_json::Value::Object(metadata)))
}

fn setup_registration_replay(
    existing_metadata: &Option<serde_json::Value>,
    incoming_metadata: &Option<serde_json::Value>,
) -> SetupRegistrationReplay {
    let (Some(existing), Some(incoming)) = (
        existing_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object),
        incoming_metadata
            .as_ref()
            .and_then(serde_json::Value::as_object),
    ) else {
        return SetupRegistrationReplay::None;
    };
    let Some(operation_id) = existing
        .get(SETUP_REGISTRATION_OPERATION_ID)
        .and_then(serde_json::Value::as_str)
    else {
        return SetupRegistrationReplay::None;
    };
    if incoming
        .get(SETUP_REGISTRATION_OPERATION_ID)
        .and_then(serde_json::Value::as_str)
        != Some(operation_id)
    {
        return SetupRegistrationReplay::None;
    }

    if existing.get(SETUP_REGISTRATION_IMPORT_HASH) == incoming.get(SETUP_REGISTRATION_IMPORT_HASH)
    {
        SetupRegistrationReplay::Exact
    } else {
        SetupRegistrationReplay::Conflict
    }
}

fn merge_setup_metadata(
    existing: &Option<serde_json::Value>,
    incoming: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (existing, incoming) {
        (Some(serde_json::Value::Object(existing)), Some(serde_json::Value::Object(incoming))) => {
            let mut merged = existing.clone();
            merged.extend(incoming);
            Some(serde_json::Value::Object(merged))
        }
        (_, Some(incoming)) => Some(incoming),
        (existing, None) => existing.clone(),
    }
}

fn can_accept_reimport(
    existing: &DeploymentRecord,
    imported_stack_state: &StackState,
    release_id: &str,
    req: &StackImportRequest,
) -> bool {
    let idempotent = existing.current_release_id.as_deref() == Some(release_id)
        && existing.setup_fingerprint.as_deref() == Some(req.setup_fingerprint.as_str())
        && imported_resources_are_unchanged(existing, imported_stack_state);

    matches!(
        existing.status.as_str(),
        "running" | "update-failed" | "refresh-failed"
    ) || idempotent
}

fn can_reconcile_after_import(existing: &DeploymentRecord) -> bool {
    matches!(
        existing.status.as_str(),
        "running" | "update-failed" | "refresh-failed"
    )
}

fn initial_import_status(prepared_stack: &Stack, stack_state: &StackState) -> String {
    let has_pending_setup = prepared_stack
        .resources()
        .filter(|(_, entry)| entry.lifecycle == ResourceLifecycle::Frozen)
        .any(|(resource_id, _)| {
            stack_state
                .resources
                .get(resource_id)
                .is_none_or(|resource| resource.status != ResourceStatus::Running)
        });

    if has_pending_setup {
        deployment_status_string(DeploymentStatus::InitialSetup)
    } else {
        deployment_status_string(DeploymentStatus::Provisioning)
    }
}

fn deployment_status_string(status: DeploymentStatus) -> String {
    match status {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::PreflightsFailed => "preflights-failed",
        DeploymentStatus::InitialSetup => "initial-setup",
        DeploymentStatus::InitialSetupFailed => "initial-setup-failed",
        DeploymentStatus::Provisioning => "provisioning",
        DeploymentStatus::WaitingForMachines => "waiting-for-machines",
        DeploymentStatus::ProvisioningFailed => "provisioning-failed",
        DeploymentStatus::Running => "running",
        DeploymentStatus::RefreshFailed => "refresh-failed",
        DeploymentStatus::UpdatePending => "update-pending",
        DeploymentStatus::Updating => "updating",
        DeploymentStatus::UpdateFailed => "update-failed",
        DeploymentStatus::DeletePending => "delete-pending",
        DeploymentStatus::Deleting => "deleting",
        DeploymentStatus::DeleteFailed => "delete-failed",
        DeploymentStatus::TeardownRequired => "teardown-required",
        DeploymentStatus::TeardownFailed => "teardown-failed",
        DeploymentStatus::Deleted => "deleted",
        DeploymentStatus::Error => "error",
    }
    .to_string()
}

fn import_changes_deployment(
    existing: &DeploymentRecord,
    imported_stack_state: &StackState,
    environment_info: &Option<EnvironmentInfo>,
    runtime_metadata: &RuntimeMetadata,
    release_id: &str,
    req: &StackImportRequest,
) -> bool {
    existing.current_release_id.as_deref() != Some(release_id)
        || existing.setup_target.as_deref() != Some(req.setup_target.as_str())
        || existing.setup_fingerprint.as_deref() != Some(req.setup_fingerprint.as_str())
        || existing.setup_fingerprint_version != Some(req.setup_fingerprint_version)
        || existing.stack_settings.as_ref() != Some(&req.stack_settings)
        || existing.environment_info.as_ref() != environment_info.as_ref()
        || existing.runtime_metadata.as_ref() != Some(runtime_metadata)
        // An edited gate answer changes nothing else in the payload, but the
        // reconcile it schedules is what provisions or deprovisions the
        // gated resource.
        || existing.input_values != req.input_values
        || !imported_resources_are_unchanged(existing, imported_stack_state)
}

fn imported_resources_are_unchanged(
    existing: &DeploymentRecord,
    imported_stack_state: &StackState,
) -> bool {
    let Some(existing_stack_state) = existing.stack_state.as_ref() else {
        return false;
    };

    imported_stack_state.resources.iter().all(|(id, imported)| {
        existing_stack_state
            .resources
            .get(id)
            .is_some_and(|existing| {
                serde_json::to_value(existing).ok() == serde_json::to_value(imported).ok()
            })
    })
}

fn import_runtime_metadata(stack: &Stack) -> RuntimeMetadata {
    RuntimeMetadata {
        prepared_stack: Some(stack.clone()),
        ..RuntimeMetadata::default()
    }
}

fn reimport_runtime_metadata(
    existing: &DeploymentRecord,
    prepared_stack: &Stack,
    release_id: &str,
    req: &StackImportRequest,
) -> crate::error::Result<RuntimeMetadata> {
    let mut metadata = existing.runtime_metadata.clone().unwrap_or_default();
    let baseline_stack = metadata.prepared_stack.as_ref().ok_or_else(|| {
        AlienError::new(ErrorData::ImportedDeploymentConflict {
            reason: format!(
                "Imported deployment '{}' has no successful prepared stack; complete or repair its current deployment before rerunning setup",
                existing.id
            ),
        })
    })?;
    let baseline_frozen_digest = baseline_stack.frozen_resources_digest();
    let target_frozen_digest = prepared_stack.frozen_resources_digest();

    metadata.setup_update_authorization =
        (baseline_frozen_digest != target_frozen_digest).then(|| SetupUpdateAuthorization {
            nonce: uuid::Uuid::new_v4().to_string(),
            baseline_frozen_digest,
            target_frozen_digest,
            release_id: release_id.to_string(),
            setup_target: req.setup_target.clone(),
            setup_fingerprint: req.setup_fingerprint.clone(),
            setup_fingerprint_version: req.setup_fingerprint_version,
        });
    Ok(metadata)
}

async fn prepare_import_stack(
    source_stack: Stack,
    req: &StackImportRequest,
) -> crate::error::Result<Stack> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    let mutation_platform = req.platform;
    runner
        .run_template_preflights(&source_stack, mutation_platform)
        .await
        .map_err(|err| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Source stack failed setup import preflights: {}",
                    err.message
                ),
            })
        })?;

    let stack_state = StackState::new(mutation_platform);
    let config = DeploymentConfig {
        input_values: Default::default(),
        deployment_name: Some(req.deployment_name.clone()),
        stack_settings: req.stack_settings.clone(),
        management_config: req.management_config.clone(),
        environment_variables: EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        },
        allow_frozen_changes: false,
        compute_backend: None,
        external_bindings: ExternalBindings::default(),
        base_platform: req.base_platform,
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        public_endpoints: None,
        domain_metadata: None,
        monitoring: None,
        manager_url: None,
        deployment_token: None,
        native_image_host: None,
    };

    runner
        .apply_mutations(source_stack, &stack_state, &config)
        .await
        .map_err(|err| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Failed to derive expected setup stack from import settings: {}",
                    err.message
                ),
            })
        })
}

fn infer_import_environment_info(req: &StackImportRequest) -> Option<EnvironmentInfo> {
    match req.base_platform.unwrap_or(req.platform) {
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

/// Look up the stack for the runtime platform being imported. For managed
/// Kubernetes, `basePlatform` selects the setup cloud and importers, but the
/// release stack is still keyed by `kubernetes`.
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
/// gets enough context to fix the setup artifact without log spelunking.
fn build_stack_state(
    state: &AppState,
    _subject: &Subject,
    req: &StackImportRequest,
    stack: &Stack,
) -> crate::error::Result<StackState> {
    let mut resources: HashMap<String, StackResourceState> = HashMap::new();

    for imported in &req.resources {
        let import_platform = import_platform_for_resource(state, req, &imported.resource_type);
        let entry = stack.resources.get(&imported.id).ok_or_else(|| {
            AlienError::new(ErrorData::BadRequest {
                reason: format!(
                    "Imported resource '{}' is not present in the active stack \
                     for platform '{}' — release the stack before importing it",
                    imported.id, import_platform
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
        let mut resource_state = state
            .import_registry
            .run(
                &imported.resource_type,
                import_platform,
                imported.import_data.clone(),
                &ImportContext {
                    resource_id: &imported.id,
                    platform: import_platform,
                    region: &req.region,
                    stack_settings: &req.stack_settings,
                    management_config: req.management_config.as_ref(),
                    resource: entry,
                },
            )
            .map_err(|err| {
                AlienError::new(ErrorData::BadRequest {
                    reason: format!(
                        "Failed to import resource '{}' (type='{}', platform='{}'): {}",
                        imported.id, imported.resource_type, import_platform, err.message
                    ),
                })
            })?;
        resource_state.controller_platform = Some(import_platform);

        resources.insert(imported.id.clone(), resource_state);
    }

    let mut stack_state =
        StackState::with_resource_prefix(req.platform, req.resource_prefix.trim().to_string());
    stack_state.resources = resources;
    Ok(stack_state)
}

fn import_platform_for_resource(
    state: &AppState,
    req: &StackImportRequest,
    resource_type: &alien_core::ResourceType,
) -> Platform {
    if req.platform != Platform::Kubernetes {
        return req.base_platform.unwrap_or(req.platform);
    }

    if resource_type == &KubernetesCluster::RESOURCE_TYPE {
        return req.base_platform.unwrap_or(Platform::Kubernetes);
    }

    if let Some(base_platform) = req.base_platform {
        if state
            .import_registry
            .importer(resource_type, base_platform)
            .is_some()
        {
            return base_platform;
        }
    }

    Platform::Kubernetes
}

#[cfg(test)]
mod setup_update_authorization_tests {
    use super::*;
    use alien_core::{Storage, Worker, WorkerCode};
    use chrono::Utc;

    fn request() -> StackImportRequest {
        StackImportRequest {
            setup_import_format_version: CURRENT_SETUP_IMPORT_FORMAT_VERSION,
            deployment_group_token: String::new(),
            deployment_name: "deployment".to_string(),
            resource_prefix: "deployment".to_string(),
            source_kind: None,
            setup_metadata: None,
            release_id: Some("release".to_string()),
            platform: Platform::Aws,
            base_platform: None,
            region: "region".to_string(),
            setup_target: "target".to_string(),
            setup_fingerprint: "fingerprint".to_string(),
            setup_fingerprint_version: 1,
            stack_settings: Default::default(),
            management_config: None,
            input_values: HashMap::new(),
            resources: vec![],
        }
    }

    fn record(prepared_stack: Stack) -> DeploymentRecord {
        DeploymentRecord {
            id: "deployment".to_string(),
            workspace_id: "workspace".to_string(),
            project_id: "project".to_string(),
            name: "deployment".to_string(),
            deployment_group_id: "group".to_string(),
            platform: Platform::Aws,
            deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            base_platform: None,
            status: "running".to_string(),
            stack_settings: Some(Default::default()),
            stack_state: Some(StackState::new(Platform::Aws)),
            environment_info: None,
            runtime_metadata: Some(RuntimeMetadata {
                prepared_stack: Some(prepared_stack),
                last_synced_env_vars_hash: Some("env-hash".to_string()),
                registry_access_granted: true,
                ..RuntimeMetadata::default()
            }),
            current_release_id: Some("release".to_string()),
            desired_release_id: Some("release".to_string()),
            import_source: None,
            setup_method: None,
            setup_metadata: None,
            setup_target: Some("target".to_string()),
            setup_fingerprint: Some("fingerprint".to_string()),
            setup_fingerprint_version: Some(1),
            user_environment_variables: None,
            management_config: None,
            deployment_config: None,
            deployment_token: None,
            input_values: Default::default(),
            retry_requested: false,
            locked_by: None,
            locked_at: None,
            created_at: Utc::now(),
            updated_at: None,
            error: None,
        }
    }

    fn stack(live_id: &str, frozen_id: &str) -> Stack {
        Stack::new("stack".to_string())
            .add(
                Storage::new(frozen_id.to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Worker::new(live_id.to_string())
                    .code(WorkerCode::Image {
                        image: "image".to_string(),
                    })
                    .permissions("default".to_string())
                    .build(),
                ResourceLifecycle::Live,
            )
            .build()
    }

    #[test]
    fn setup_registration_replay_is_exact_for_the_same_operation_and_payload() {
        let mut req = request();
        req.setup_metadata = Some(serde_json::json!({
            SETUP_REGISTRATION_OPERATION_ID: "operation-1",
        }));
        let metadata = setup_metadata_for_persistence(&req).expect("metadata should hash");

        assert_eq!(
            setup_registration_replay(&metadata, &metadata),
            SetupRegistrationReplay::Exact
        );
    }

    /// A re-register that only flips a gate answer changes nothing else in
    /// the payload; without the input-values clause the deployment never
    /// reconciles and the toggle silently does nothing.
    #[test]
    fn an_input_only_edit_schedules_reconciliation() {
        let prepared = stack("live-worker", "frozen-storage");
        let existing = record(prepared);
        let mut req = request();
        req.stack_settings = existing.stack_settings.clone().unwrap();
        let runtime_metadata = existing.runtime_metadata.clone().unwrap();
        let imported_state = StackState::new(Platform::Aws);

        assert!(!import_changes_deployment(
            &existing,
            &imported_state,
            &None,
            &runtime_metadata,
            "release",
            &req,
        ));

        req.input_values
            .insert("enableAnalytics".to_string(), serde_json::json!(false));
        assert!(import_changes_deployment(
            &existing,
            &imported_state,
            &None,
            &runtime_metadata,
            "release",
            &req,
        ));
    }

    #[test]
    fn setup_registration_replay_conflicts_when_the_payload_changes() {
        let mut original = request();
        original.setup_metadata = Some(serde_json::json!({
            SETUP_REGISTRATION_OPERATION_ID: "operation-1",
        }));
        let original_metadata =
            setup_metadata_for_persistence(&original).expect("metadata should hash");
        let mut changed = original;
        changed.resource_prefix = "different-prefix".to_string();
        let changed_metadata =
            setup_metadata_for_persistence(&changed).expect("metadata should hash");

        assert_eq!(
            setup_registration_replay(&original_metadata, &changed_metadata),
            SetupRegistrationReplay::Conflict
        );
    }

    #[test]
    fn setup_registration_replay_ignores_a_different_operation() {
        let mut original = request();
        original.setup_metadata = Some(serde_json::json!({
            SETUP_REGISTRATION_OPERATION_ID: "operation-1",
        }));
        let original_metadata =
            setup_metadata_for_persistence(&original).expect("metadata should hash");
        let mut next = original;
        next.setup_metadata = Some(serde_json::json!({
            SETUP_REGISTRATION_OPERATION_ID: "operation-2",
        }));
        let next_metadata = setup_metadata_for_persistence(&next).expect("metadata should hash");

        assert_eq!(
            setup_registration_replay(&original_metadata, &next_metadata),
            SetupRegistrationReplay::None
        );
    }

    #[test]
    fn no_op_and_live_only_reimports_do_not_mint_setup_authority() {
        let baseline = stack("live-a", "frozen");
        for target in [baseline.clone(), stack("live-b", "frozen")] {
            let metadata = reimport_runtime_metadata(
                &record(baseline.clone()),
                &target,
                "release",
                &request(),
            )
            .expect("stable setup import should succeed");

            assert!(metadata.setup_update_authorization.is_none());
            assert_eq!(
                metadata.last_synced_env_vars_hash.as_deref(),
                Some("env-hash")
            );
            assert!(metadata.registry_access_granted);
        }
    }

    #[test]
    fn frozen_reimport_mints_exact_authority_without_losing_runtime_metadata() {
        let baseline = stack("live", "frozen-a");
        let target = stack("live", "frozen-b");
        let metadata =
            reimport_runtime_metadata(&record(baseline.clone()), &target, "release", &request())
                .expect("setup-owned update should succeed");
        let authorization = metadata
            .setup_update_authorization
            .expect("frozen change should mint setup authority");

        assert_eq!(
            authorization.baseline_frozen_digest,
            baseline.frozen_resources_digest()
        );
        assert_eq!(
            authorization.target_frozen_digest,
            target.frozen_resources_digest()
        );
        assert_eq!(authorization.release_id, "release");
        assert_eq!(
            metadata.last_synced_env_vars_hash.as_deref(),
            Some("env-hash")
        );
        assert!(metadata.registry_access_granted);
    }
}
