//! CLI commands for operations plugins.
//!
//! Operations plugins package named operations (`plugin/operation`) that run
//! inside a deployment via the commands interface. `publish` uploads a custom
//! plugin bundle (a ZIP with `metadata.json` + per-arch binaries) to the
//! platform so a workspace can use its operations; `list` shows the catalog.
//!
//! Platform-gated: these talk to the Alien platform API, not a standalone
//! manager.

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use alien_commands_client::{CommandsClient, CommandsClientConfig};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_platform_api::types::{
    InvokeOperationRequest, InvokeOperationResponseStatus, VerifyOperationCheckRequest,
    VerifyOperationCheckResponseOutcome,
};
use alien_platform_api::SdkResultExt as _;
use clap::{Parser, Subcommand};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{ErrorData, Result};
use crate::execution_context::ExecutionMode;
use crate::output::print_json;

#[derive(Parser, Debug, Clone)]
#[command(
    about = "Manage operations plugins",
    long_about = "Manage operations plugins.

Operations plugins package named operations you can run inside a deployment via
the commands interface (`plugin/operation`). Publish a custom bundle to make its
operations available in your workspace.

EXAMPLES:
    # Publish a custom plugin bundle
    alien operations publish ./postgres-operations-1.0.0.zip

    # List available plugins (builtin + custom)
    alien operations list

    # Invoke an enabled operation without an AI agent
    alien operations invoke --deployment mycustomer/prod \\
      --operation kubernetes/get-pods \\
      --params '{\"namespace\": \"default\", \"maxResults\": 10}'
"
)]
pub struct OperationsArgs {
    #[command(subcommand)]
    pub action: OperationsAction,

    /// Project ID or name. Defaults to the linked project.
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum OperationsAction {
    /// Publish a custom operations plugin bundle (ZIP) to your workspace.
    Publish {
        /// Path to the plugin bundle ZIP (contains metadata.json + binaries).
        bundle: PathBuf,
    },
    /// List available operations plugins (builtin + custom).
    List,
    /// Invoke an enabled operation and wait for its result.
    Invoke {
        /// Deployment ID, or <deployment-group-name>/<deployment-name>.
        #[arg(long)]
        deployment: String,

        /// Operation name in <plugin>/<operation> form.
        #[arg(long)]
        operation: String,

        /// Operation parameters as JSON.
        #[arg(long, default_value = "{}")]
        params: String,

        /// Timeout in seconds.
        #[arg(long, default_value = "60")]
        timeout: u64,

        /// If the operation requires approval, automatically create an access
        /// request instead of printing instructions.
        #[arg(long = "request-access")]
        request_access: bool,

        /// Approval duration to request with --request-access, e.g. 1h, 30m.
        #[arg(long = "access-duration", default_value = "1h")]
        access_duration: String,
    },
}

/// The `metadata.json` fields the CLI reads to describe the bundle. The full
/// object is forwarded to the platform, which validates it authoritatively.
#[derive(Debug, Deserialize)]
struct BundleMetadata {
    name: String,
    version: String,
    #[serde(default)]
    tier: Option<String>,
}

/// Step 1: ask the platform for a presigned S3 URL to upload the bundle ZIP to.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadUrlRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadUrlResponse {
    /// Presigned S3 PUT URL to upload the ZIP to.
    upload_url: String,
    /// Content-Type header the PUT must send (must match the presign signature).
    content_type: String,
}

/// Step 2 (after the S3 upload): register the plugin. The bytes are already in
/// S3; only metadata travels here.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishRequest {
    name: String,
    version: String,
    tier: String,
    /// The full, verbatim metadata.json object (platform re-validates it).
    metadata: Value,
}

struct InvokeTaskOptions<'a> {
    deployment: &'a str,
    operation: &'a str,
    params: &'a str,
    timeout_secs: u64,
    json: bool,
    request_access: bool,
    access_duration: &'a str,
}

pub async fn operations_task(args: OperationsArgs, ctx: ExecutionMode) -> Result<()> {
    let auth = ctx.auth_http().await?;
    let workspace = ctx.resolve_workspace_with_bootstrap(!args.json).await?;
    // The operations catalog is project-scoped: the platform requires a
    // `project` alongside `workspace`. Resolve the linked project (or the
    // `--project` override) the same way the other project-scoped commands do.
    let (_, project_link) = ctx
        .resolve_project(args.project.as_deref(), !args.json)
        .await?;
    let project = project_link.project_id;

    match args.action {
        OperationsAction::Publish { bundle } => {
            publish_task(&auth, &workspace, &project, &bundle, args.json).await
        }
        OperationsAction::List => list_task(&auth, &workspace, &project, args.json).await,
        OperationsAction::Invoke {
            deployment,
            operation,
            params,
            timeout,
            request_access,
            access_duration,
        } => {
            invoke_task(
                &ctx,
                &workspace,
                &project,
                InvokeTaskOptions {
                    deployment: &deployment,
                    operation: &operation,
                    params: &params,
                    timeout_secs: timeout,
                    json: args.json,
                    request_access,
                    access_duration: &access_duration,
                },
            )
            .await
        }
    }
}

async fn invoke_task(
    ctx: &ExecutionMode,
    workspace: &str,
    project: &str,
    options: InvokeTaskOptions<'_>,
) -> Result<()> {
    let operation_ref = options.operation;
    let (plugin, operation) = parse_operation_reference(operation_ref).ok_or_else(|| {
        AlienError::new(ErrorData::ValidationError {
            field: "operation".to_string(),
            message: "Use <plugin>/<operation>, for example kubernetes/get-pods.".to_string(),
        })
    })?;
    let params: Value = serde_json::from_str(options.params)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "params".to_string(),
            message: "Invalid JSON".to_string(),
        })?;
    let resolved = crate::platform_deployment_resolver::resolve_with_manager(
        ctx,
        options.deployment,
        Some(project),
        !options.json,
    )
    .await?;
    let deployment_id = String::from(resolved.detail.id.clone());

    let sdk_client = ctx.sdk_client().await?;
    let invocation = sdk_client
        .invoke_operation()
        .workspace(workspace)
        .project(project)
        .body(InvokeOperationRequest {
            deployment_id: deployment_id.clone(),
            plugin: plugin.to_string(),
            operation: operation.to_string(),
            params: Some(params),
            remediation_plan_id: None,
            access_request_id: None,
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("invoking operation '{operation_ref}'"),
            url: None,
        })?
        .into_inner();

    let invocation = if invocation.status == InvokeOperationResponseStatus::PendingApproval {
        if !options.request_access {
            if options.json {
                print_json(&invocation)?;
            } else {
                println!(
                    "Access is required to run {operation_ref}.\n\n\
                     Request access:\n\
                     \x20\x20alien access-requests create \\\n\
                     \x20\x20\x20\x20--deployment {} \\\n\
                     \x20\x20\x20\x20--operation {operation_ref} \\\n\
                     \x20\x20\x20\x20--params '{}' \\\n\
                     \x20\x20\x20\x20--duration 1h\n\n\
                     Or rerun this command with --request-access.",
                    options.deployment, options.params,
                );
            }
            return Ok(());
        }

        request_access_then_reinvoke(
            &sdk_client,
            workspace,
            project,
            &deployment_id,
            plugin,
            operation,
            options.params,
            options.access_duration,
        )
        .await?
    } else {
        invocation
    };

    let command_id = invocation.command_id.ok_or_else(|| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "operation '{operation_ref}' reported '{}' without a command ID",
                invocation.status
            ),
            url: None,
        })
    })?;
    let commands_url = format!("{}/v1", resolved.manager.manager_url.trim_end_matches('/'));
    let client = CommandsClient::with_http_client(
        &commands_url,
        &deployment_id,
        resolved.manager.http_client,
        CommandsClientConfig {
            timeout: Duration::from_secs(options.timeout_secs),
            ..Default::default()
        },
    );
    let result: Value = client
        .wait_for_completion(&command_id)
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("operation '{operation_ref}' failed"),
            url: Some(commands_url),
        })?;

    // The write already ran and `result` is its real outcome — a failure to
    // even check verification (network blip, API error) must not be reported
    // as the operation itself failing, or a caller could retry an
    // already-applied write. Degrade to Unverified instead of propagating.
    let verification = match verify_operation(
        &sdk_client,
        workspace,
        project,
        &deployment_id,
        plugin,
        operation,
        &result,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(err) => VerificationOutcome::Unverified {
            reason: format!("could not check verification: {err}"),
        },
    };

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "result": result,
                "verification": verification,
            }))
            .unwrap_or_else(|_| format!("{result:?}"))
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{result:?}"))
        );
        match &verification {
            VerificationOutcome::Verified => {
                println!("\nVerified: the change took effect.");
            }
            VerificationOutcome::Unverified { reason } => {
                println!(
                    "\nDispatched, but could not confirm the change took effect: {reason}\n\
                     The write ran; this only means Alien couldn't verify its result. Check manually."
                );
            }
            VerificationOutcome::Skipped { .. } => {
                // No verification declared, or the caller didn't opt in — say
                // nothing; this is the same as every operation before
                // verification existed, not a new failure mode to announce.
            }
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum VerificationOutcome {
    Verified,
    Unverified { reason: String },
    Skipped { reason: String },
}

/// `--request-access`: create an exact access request for the denied
/// operation, wait for it to be approved, then re-invoke with the resulting
/// `access_request_id` so it dispatches under that approval instead of the
/// project's default policy. Blocks until approved or the wait times out —
/// same 1-hour default as `access-requests wait`, since there is no
/// separate timeout flag for this shortcut path.
#[allow(clippy::too_many_arguments)]
async fn request_access_then_reinvoke(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    project: &str,
    deployment_id: &str,
    plugin: &str,
    operation: &str,
    params_json: &str,
    access_duration: &str,
) -> Result<alien_platform_api::types::InvokeOperationResponse> {
    let params: Option<Value> = Some(
        serde_json::from_str(params_json)
            .into_alien_error()
            .context(ErrorData::ValidationError {
                field: "params".to_string(),
                message: "Invalid JSON".to_string(),
            })?,
    );
    // Requested duration is informational on create (the approver sets the
    // real grant window on approve) — reuse the same parser only to fail
    // fast on an obviously malformed --access-duration before creating
    // anything.
    let _ = crate::commands::access_requests::parse_duration_minutes(access_duration)?;

    let created = sdk_client
        .create_access_request()
        .workspace(workspace)
        .body(alien_platform_api::types::CreateAccessRequest {
            deployment_id: deployment_id.to_string(),
            operation: Some(format!("{plugin}/{operation}")),
            params,
            operation_pattern: None,
            max_risk: None,
            title: None,
            reason: None,
            remediation_plan_id: None,
            commands: Vec::new(),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: "creating access request".to_string(),
            url: None,
        })?
        .into_inner();

    // Progress (this message, the approve command once available) always
    // goes to stderr. In --json mode, stdout is reserved for exactly one
    // JSON document — the final result — so a caller can pipe it straight
    // into a parser; printing progress there too (even as separate JSON
    // objects) would make stdout unparseable as a single value. Same idea
    // as `docker pull` writing progress to stderr and the final digest to
    // stdout.
    eprintln!(
        "Access requested: {}\nWaiting for the customer to approve it in-cluster...",
        created.id
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(3600);
    let mut printed_kubectl_approve = false;
    loop {
        let request = sdk_client
            .get_access_request()
            .id(created.id.as_str())
            .workspace(workspace)
            .send()
            .await
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("waiting for access request '{}'", created.id),
                url: None,
            })?
            .into_inner();

        if !printed_kubectl_approve {
            let kubectl_approve = crate::commands::access_requests::fetch_kubectl_approve(
                sdk_client,
                workspace,
                created.id.as_str(),
            )
            .await?;
            if let Some(command) = &kubectl_approve {
                eprintln!("Run this in-cluster to approve:\n  {command}\n");
                printed_kubectl_approve = true;
            }
        }

        match request.status {
            alien_platform_api::types::AccessRequestStatus::CustomerApproved => break,
            alien_platform_api::types::AccessRequestStatus::Rejected
            | alien_platform_api::types::AccessRequestStatus::Expired => {
                return Err(AlienError::new(ErrorData::ApiRequestFailed {
                    message: format!(
                        "access request '{}' is '{}', not approved",
                        created.id, request.status
                    ),
                    url: None,
                }));
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            return Err(AlienError::new(ErrorData::ApiRequestFailed {
                message: format!(
                    "timed out waiting for access request '{}' to be approved",
                    created.id
                ),
                url: None,
            }));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    let reinvoke_params: Value = serde_json::from_str(params_json)
        .into_alien_error()
        .context(ErrorData::ValidationError {
            field: "params".to_string(),
            message: "Invalid JSON".to_string(),
        })?;
    sdk_client
        .invoke_operation()
        .workspace(workspace)
        .project(project)
        .body(InvokeOperationRequest {
            deployment_id: deployment_id.to_string(),
            plugin: plugin.to_string(),
            operation: operation.to_string(),
            params: Some(reinvoke_params),
            remediation_plan_id: None,
            access_request_id: Some(created.id.to_string()),
        })
        .send()
        .await
        .into_sdk_error()
        .context(ErrorData::ApiRequestFailed {
            message: format!("invoking operation '{plugin}/{operation}' after approval"),
            url: None,
        })
        .map(|response| response.into_inner())
}

/// Poll `POST /v1/operations/verify-check` per the operation's own declared
/// retry policy (echoed back on each response) until verified, the poll
/// operation itself fails, or the declared timeout elapses. Alert-only: on
/// timeout or poll failure this returns `Unverified`, never attempts a
/// rollback — the underlying write already ran; verification only confirms
/// whether Alien could tell it worked.
///
/// Testing note: this function's only real behavior is HTTP calls against
/// the platform API's verify-check endpoint plus a sleep/timeout loop around
/// them — mocking `alien_platform_api::Client` here would mean asserting our
/// own mock's scripted responses, not real behavior. The outcome-mapping
/// logic (`Skipped`/`Verified`/`Failed`/`NotYet` → `VerificationOutcome`) is
/// exercised for real in `apps/api/src/routes/operations.test.ts`'s
/// verify-check tests, against a real Postgres via Testcontainers. To
/// validate this loop end to end: deploy a kind cluster with the kubernetes
/// operations plugin, run `alien operations invoke kubernetes/restart-pod
/// --params '{"pod":"...","verifyWorkload":"deployment/..."}'`, and confirm
/// it prints "Verified: the change took effect." once the replacement pod is
/// ready — then repeat against a Deployment that cannot schedule its
/// replacement and confirm it reports the timeout message instead of a false
/// "Verified".
async fn verify_operation(
    sdk_client: &alien_platform_api::Client,
    workspace: &str,
    project: &str,
    deployment_id: &str,
    plugin: &str,
    operation: &str,
    write_result: &Value,
) -> Result<VerificationOutcome> {
    // The declared verification timeout covers this whole function, not just
    // the retry loop below — start the clock before the FIRST verify-check
    // call, not after it returns. Starting it later would let a slow (but
    // still within-bound) initial call add its own latency on top of the
    // declared timeout instead of counting against it.
    let start = std::time::Instant::now();

    // This first call is the one that DECLARES the timeout/retry policy — we
    // can't bound it by "what's left of the timeout" since that isn't known
    // yet. Bound it by a fixed ceiling instead, generous enough to allow for
    // real network latency over the API's own ~9s request watchdog, so a
    // slow or stalled response can't block the CLI indefinitely after the
    // write already completed.
    const INITIAL_VERIFY_CHECK_TIMEOUT: Duration = Duration::from_secs(15);
    let deadline_check = tokio::time::timeout(
        INITIAL_VERIFY_CHECK_TIMEOUT,
        sdk_client
            .verify_operation_check()
            .workspace(workspace)
            .project(project)
            .body(VerifyOperationCheckRequest {
                deployment_id: deployment_id.to_string(),
                plugin: plugin.to_string(),
                operation: operation.to_string(),
                write_result: Some(write_result.clone()),
            })
            .send(),
    )
    .await
    .map_err(|_| {
        AlienError::new(ErrorData::ApiRequestFailed {
            message: format!(
                "checking verification for '{plugin}/{operation}' timed out after {}s",
                INITIAL_VERIFY_CHECK_TIMEOUT.as_secs()
            ),
            url: None,
        })
    })?
    .into_sdk_error()
    .context(ErrorData::ApiRequestFailed {
        message: format!("checking verification for '{plugin}/{operation}'"),
        url: None,
    })?
    .into_inner();

    match deadline_check.outcome {
        VerifyOperationCheckResponseOutcome::Skipped => {
            return Ok(VerificationOutcome::Skipped {
                reason: deadline_check
                    .reason
                    .unwrap_or_else(|| "no verification declared".to_string()),
            });
        }
        VerifyOperationCheckResponseOutcome::Verified => return Ok(VerificationOutcome::Verified),
        VerifyOperationCheckResponseOutcome::Failed => {
            return Ok(VerificationOutcome::Unverified {
                reason: deadline_check
                    .reason
                    .unwrap_or_else(|| "verification check failed".to_string()),
            });
        }
        VerifyOperationCheckResponseOutcome::NotYet => {}
    }

    let Some(retry) = deadline_check.retry else {
        return Ok(VerificationOutcome::Unverified {
            reason: "not yet verified and the operation declares no retry policy".to_string(),
        });
    };
    let timeout = deadline_check
        .timeout_seconds
        .map(|secs| Duration::from_secs(secs.max(0) as u64))
        .unwrap_or(Duration::from_secs(60));
    let interval = Duration::from_secs(retry.interval_seconds.max(0) as u64);

    for _attempt in 1..retry.max_attempts.max(1) {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            break;
        }
        // Cap the sleep to what's left of the declared timeout — sleeping the
        // full interval unconditionally can overrun the timeout by up to
        // interval + request latency while still reporting only the
        // configured duration.
        let remaining = timeout - elapsed;
        tokio::time::sleep(interval.min(remaining)).await;

        // Bound the request itself by what's left of the timeout too — a
        // slow or stalled response must not keep this running past the
        // declared duration. A timeout here is treated the same as
        // "not yet verified", not an error: the write already succeeded.
        let remaining_after_sleep = timeout.saturating_sub(start.elapsed());
        if remaining_after_sleep.is_zero() {
            break;
        }
        let check_result = tokio::time::timeout(
            remaining_after_sleep,
            sdk_client
                .verify_operation_check()
                .workspace(workspace)
                .project(project)
                .body(VerifyOperationCheckRequest {
                    deployment_id: deployment_id.to_string(),
                    plugin: plugin.to_string(),
                    operation: operation.to_string(),
                    write_result: Some(write_result.clone()),
                })
                .send(),
        )
        .await;
        let Ok(send_result) = check_result else {
            break;
        };
        let check = send_result
            .into_sdk_error()
            .context(ErrorData::ApiRequestFailed {
                message: format!("checking verification for '{plugin}/{operation}'"),
                url: None,
            })?
            .into_inner();

        match check.outcome {
            VerifyOperationCheckResponseOutcome::Verified => return Ok(VerificationOutcome::Verified),
            VerifyOperationCheckResponseOutcome::Failed => {
                return Ok(VerificationOutcome::Unverified {
                    reason: check
                        .reason
                        .unwrap_or_else(|| "verification check failed".to_string()),
                });
            }
            VerifyOperationCheckResponseOutcome::Skipped => {
                return Ok(VerificationOutcome::Skipped {
                    reason: check
                        .reason
                        .unwrap_or_else(|| "no verification declared".to_string()),
                });
            }
            VerifyOperationCheckResponseOutcome::NotYet => {}
        }
    }

    Ok(VerificationOutcome::Unverified {
        reason: format!(
            "timed out after {}s waiting for the change to be confirmed",
            timeout.as_secs()
        ),
    })
}

fn parse_operation_reference(reference: &str) -> Option<(&str, &str)> {
    let (plugin, operation) = reference.split_once('/')?;
    if plugin.is_empty() || operation.is_empty() || operation.contains('/') {
        return None;
    }
    Some((plugin, operation))
}

/// Read + validate the bundle, upload the ZIP straight to S3 via a presigned
/// URL the platform mints, then register the plugin. The bytes never flow
/// through the API.
async fn publish_task(
    auth: &crate::auth::AuthHttp,
    workspace: &str,
    project: &str,
    bundle_path: &PathBuf,
    json: bool,
) -> Result<()> {
    let bytes =
        std::fs::read(bundle_path)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("could not read bundle '{}'", bundle_path.display()),
            })?;

    let (metadata_value, metadata) = read_bundle_metadata(&bytes, bundle_path)?;
    let tier = metadata
        .tier
        .clone()
        .unwrap_or_else(|| "destructive".to_string());

    // Step 1: get a presigned S3 PUT URL for this plugin's bundle.
    let upload_url_endpoint = api_url(
        &auth.base_url,
        "/v1/operations/plugins/upload-url",
        workspace,
        project,
    )?;
    let presign_response = auth
        .reqwest_client()
        .request(Method::POST, upload_url_endpoint.clone())
        .json(&UploadUrlRequest {
            name: metadata.name.clone(),
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "requesting a bundle upload URL".to_string(),
            url: Some(upload_url_endpoint.to_string()),
        })?;
    if !presign_response.status().is_success() {
        let status = presign_response.status();
        let body = presign_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("could not get upload URL ({status}): {body}"),
            url: Some(upload_url_endpoint.to_string()),
        }));
    }
    let presign: UploadUrlResponse =
        presign_response
            .json()
            .await
            .into_alien_error()
            .context(ErrorData::ApiRequestFailed {
                message: "parsing the bundle upload URL response".to_string(),
                url: Some(upload_url_endpoint.to_string()),
            })?;

    // Step 2: PUT the ZIP directly to S3. The Content-Type MUST match what the
    // presign was signed with, or S3 rejects the signature.
    let put_response = auth
        .reqwest_client()
        .request(Method::PUT, &presign.upload_url)
        .header("content-type", &presign.content_type)
        .body(bytes)
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "uploading the bundle to storage".to_string(),
            url: Some(presign.upload_url.clone()),
        })?;
    if !put_response.status().is_success() {
        let status = put_response.status();
        let body = put_response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("bundle upload failed ({status}): {body}"),
            url: Some(presign.upload_url.clone()),
        }));
    }

    // Step 3: register the plugin now that its ZIP is in S3.
    let publish_endpoint = api_url(&auth.base_url, "/v1/operations/plugins", workspace, project)?;
    let response = auth
        .reqwest_client()
        .request(Method::POST, publish_endpoint.clone())
        .json(&PublishRequest {
            name: metadata.name.clone(),
            version: metadata.version.clone(),
            tier,
            metadata: metadata_value,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "publishing operations plugin".to_string(),
            url: Some(publish_endpoint.to_string()),
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("publish failed ({status}): {body}"),
            url: Some(publish_endpoint.to_string()),
        }));
    }

    if json {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    } else {
        println!(
            "Published plugin '{}' v{} to workspace '{}'.",
            metadata.name, metadata.version, workspace
        );
    }
    Ok(())
}

async fn list_task(
    auth: &crate::auth::AuthHttp,
    workspace: &str,
    project: &str,
    json: bool,
) -> Result<()> {
    let url = api_url(&auth.base_url, "/v1/operations/plugins", workspace, project)?;
    let response = auth
        .reqwest_client()
        .request(Method::GET, url.clone())
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ApiRequestFailed {
            message: "listing operations plugins".to_string(),
            url: Some(url.to_string()),
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AlienError::new(ErrorData::ApiRequestFailed {
            message: format!("list failed ({status}): {body}"),
            url: Some(url.to_string()),
        }));
    }

    let body: Value = response.json().await.unwrap_or(Value::Null);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    } else {
        let plugins = body.get("plugins").and_then(Value::as_array);
        match plugins {
            Some(plugins) if !plugins.is_empty() => {
                for p in plugins {
                    let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
                    let version = p.get("version").and_then(Value::as_str).unwrap_or("?");
                    let builtin = p.get("builtin").and_then(Value::as_bool).unwrap_or(false);
                    let tier = p.get("tier").and_then(Value::as_str).unwrap_or("?");
                    let kind = if builtin { "builtin" } else { "custom" };
                    println!("{name}  v{version}  [{kind}, {tier}]");
                }
            }
            _ => println!("No operations plugins available."),
        }
    }
    Ok(())
}

/// Extract and validate `metadata.json` from the bundle ZIP. Returns the raw
/// JSON value (forwarded verbatim) plus the fields the CLI needs.
fn read_bundle_metadata(bytes: &[u8], path: &PathBuf) -> Result<(Value, BundleMetadata)> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: format!("'{}' is not a valid ZIP bundle", path.display()),
            })?;
    let mut file = archive.by_name("metadata.json").map_err(|_| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!("bundle '{}' has no metadata.json", path.display()),
        })
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "could not read metadata.json from bundle".to_string(),
        })?;
    let value: Value = serde_json::from_str(&contents).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "metadata.json is not valid JSON".to_string(),
        },
    )?;
    let metadata: BundleMetadata = serde_json::from_value(value.clone())
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "metadata.json is missing required fields (name, version)".to_string(),
        })?;
    Ok((value, metadata))
}

fn api_url(base_url: &str, path: &str, workspace: &str, project: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base_url).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "platform base URL is invalid".to_string(),
        },
    )?;
    url.set_path(path);
    url.query_pairs_mut()
        .append_pair("workspace", workspace)
        .append_pair("project", project);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn bundle_with_metadata(meta: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("metadata.json", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(serde_json::to_vec(meta).unwrap().as_slice())
                .unwrap();
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn reads_metadata_from_bundle() {
        let meta = serde_json::json!({
            "name": "postgres-operations",
            "version": "1.0.0",
            "tier": "mutating",
            "binaries": { "amd64": "postgres-operations-linux-amd64" },
            "operations": [ { "name": "vacuum" } ]
        });
        let bytes = bundle_with_metadata(&meta);
        let (value, parsed) =
            read_bundle_metadata(&bytes, &PathBuf::from("x.zip")).expect("valid bundle");
        assert_eq!(parsed.name, "postgres-operations");
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.tier.as_deref(), Some("mutating"));
        // The full object is forwarded verbatim (operations[] preserved).
        assert!(value.get("operations").is_some());
    }

    #[test]
    fn rejects_bundle_without_metadata() {
        // A ZIP with no metadata.json.
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file("binary", SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.finish().unwrap();
        }
        let err = read_bundle_metadata(&buf, &PathBuf::from("x.zip")).expect_err("must error");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }

    #[test]
    fn rejects_metadata_missing_required_fields() {
        let meta = serde_json::json!({ "binaries": {} });
        let bytes = bundle_with_metadata(&meta);
        let err = read_bundle_metadata(&bytes, &PathBuf::from("x.zip")).expect_err("must error");
        assert_eq!(err.code, "CONFIGURATION_ERROR");
    }

    #[test]
    fn parses_plugin_and_operation_names() {
        assert_eq!(
            parse_operation_reference("kubernetes/get-pods").expect("valid operation"),
            ("kubernetes", "get-pods")
        );
    }

    #[test]
    fn rejects_invalid_operation_names() {
        for reference in ["get-pods", "/get-pods", "kubernetes/", "a/b/c"] {
            assert_eq!(parse_operation_reference(reference), None);
        }
    }
}
