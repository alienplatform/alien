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

use alien_core::{
    import::{
        ImportContext, StackImportRequest, StackImportResponse,
        CURRENT_SETUP_IMPORT_FORMAT_VERSION, MIN_SUPPORTED_SETUP_IMPORT_FORMAT_VERSION,
    },
    is_valid_resource_prefix, AwsEnvironmentInfo, AzureEnvironmentInfo, DeploymentConfig,
    DeploymentStatus, EnvironmentInfo, EnvironmentVariablesSnapshot, ExternalBindings,
    GcpEnvironmentInfo, KubernetesCluster, Platform, ResourceLifecycle, ResourceStatus,
    RuntimeMetadata, Stack, StackResourceState, StackState, RESOURCE_PREFIX_ERROR_MESSAGE,
};
use alien_error::AlienError;

use super::{auth, AppState};
use crate::auth::{Scope, Subject};
use crate::error::ErrorData;
use crate::ids;
use crate::traits::{
    CreateImportedDeploymentParams, CreateTokenParams, DeploymentRecord, ReleaseRecord, TokenType,
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

    let stack_state = match build_stack_state(&state, &subject, &req, &prepared_stack) {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let environment_info = infer_import_environment_info(&req);
    let runtime_metadata = import_runtime_metadata(&prepared_stack);

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
            if !can_accept_reimport(&existing, &stack_state, &release.id, &req) {
                return AlienError::new(ErrorData::ImportedDeploymentConflict {
                    reason: format!(
                        "Imported deployment '{}' is not in a re-importable state and the payload is not idempotent",
                        existing.id
                    ),
                })
                .into_response();
            }
            let should_reconcile = import_changes_deployment(
                &existing,
                &stack_state,
                &environment_info,
                &runtime_metadata,
                &release.id,
                &req,
            );
            let updated = match state
                .deployment_store
                .update_imported_stack_state(
                    &subject,
                    &existing.id,
                    stack_state.clone(),
                    environment_info.clone(),
                    runtime_metadata.clone(),
                    Some(release.id.clone()),
                    req.setup_target.clone(),
                    req.setup_fingerprint.clone(),
                    req.setup_fingerprint_version,
                )
                .await
            {
                Ok(d) => d,
                Err(e) => return e.into_response(),
            };

            let updated = if should_reconcile
                && can_reconcile_after_import(&existing)
                && !reconciliation_already_scheduled(&updated)
            {
                if let Err(e) = state
                    .deployment_store
                    .set_redeploy(&subject, &existing.id)
                    .await
                {
                    return e.into_response();
                }
                match state
                    .deployment_store
                    .get_deployment(&subject, &existing.id)
                    .await
                {
                    Ok(Some(d)) => d,
                    Ok(None) => {
                        return ErrorData::not_found_deployment(&existing.id).into_response();
                    }
                    Err(e) => return e.into_response(),
                }
            } else {
                updated
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
        setup_metadata: req.setup_metadata.clone(),
        setup_target: req.setup_target.clone(),
        setup_fingerprint: req.setup_fingerprint.clone(),
        setup_fingerprint_version: req.setup_fingerprint_version,
        deployment_token: Some(raw_token.clone()),
        management_config: req.management_config.clone(),
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

fn can_accept_reimport(
    existing: &DeploymentRecord,
    imported_stack_state: &StackState,
    release_id: &str,
    req: &StackImportRequest,
) -> bool {
    let idempotent = existing.current_release_id.as_deref() == Some(release_id)
        && existing.setup_fingerprint.as_deref() == Some(req.setup_fingerprint.as_str())
        && imported_resources_are_unchanged(existing, imported_stack_state);

    if matches!(
        existing.status.as_str(),
        "initial-setup" | "provisioning" | "update-pending" | "updating"
    ) {
        return true;
    }

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

fn reconciliation_already_scheduled(deployment: &DeploymentRecord) -> bool {
    matches!(
        deployment.status.as_str(),
        "provisioning" | "update-pending" | "updating"
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
        public_urls: None,
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
