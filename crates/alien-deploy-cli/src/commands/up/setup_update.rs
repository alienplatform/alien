//! Configure an exact existing setup target before acquiring its execution claim.

use super::{
    create_manager_client, create_manager_http_client, deployment_info_url, load_public_endpoints,
    load_stack_settings, push_initial_setup_targeted, resolve_base_url, resolve_token,
    DeployConfigFile, UpArgs,
};
use crate::{
    error::{ErrorData, Result},
    output,
};
use alien_core::{embedded_config::DeployCliConfig, ClientConfig, Platform};
use alien_deployment::manager_api_transport::AcquiredDeploymentPayload;
use alien_error::{AlienError, Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SetupUpdateTarget {
    deployment_id: String,
    update_operation_id: String,
    release_id: String,
    platform: Platform,
    setup_method: Option<String>,
    stack_settings: Map<String, Value>,
}

impl SetupUpdateTarget {
    fn validate_identity(
        &self,
        deployment_id: &str,
        operation_id: &str,
        release_id: &str,
    ) -> Result<()> {
        if self.deployment_id != deployment_id
            || self.update_operation_id != operation_id
            || self.release_id != release_id
            || self.platform != Platform::Machines
            || self.setup_method.as_deref() != Some("cli")
        {
            return Err(invalid_target("The authorized setup target does not match the requested deployment, operation, release, or original Machines CLI setup identity."));
        }
        Ok(())
    }

    pub(super) fn validate_claim(&self, acquired: &AcquiredDeploymentPayload) -> Result<()> {
        if acquired
            .execution_claim
            .as_ref()
            .map(|claim| claim.operation_id.as_str())
            != Some(self.update_operation_id.as_str())
        {
            return Err(invalid_target("The setup operation changed before its execution claim was acquired. Reload the exact pending target."));
        }
        if acquired.deployment.get("id").and_then(Value::as_str) != Some(&self.deployment_id) {
            return Err(invalid_target(
                "The acquired setup deployment does not match the requested identity.",
            ));
        }
        self.validate_release(
            acquired
                .deployment
                .get("desiredReleaseId")
                .and_then(Value::as_str),
        )
    }

    pub(super) fn validate_release(&self, release_id: Option<&str>) -> Result<()> {
        if release_id != Some(self.release_id.as_str()) {
            return Err(invalid_target(
                "The desired release changed before setup. Reload the exact pending target.",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupInfo {
    setup_update: SetupUpdateTarget,
    install_context: InstallContext,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallContext {
    targets: HashMap<String, InstallTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallTarget {
    platform: Platform,
    manager_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrepareRequest<'a> {
    deployment_id: &'a str,
    update_operation_id: &'a str,
    platform: Platform,
    setup_method: &'a str,
    save_for_setup: bool,
    stack_settings: &'a Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreparedTarget {
    update_operation_id: String,
    platform: Platform,
}

fn invalid_target(message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ValidationError {
        field: "setup-update".to_string(),
        message: message.to_string(),
    })
}

async fn read_response<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    if !response.status().is_success() {
        // Server bodies can include submitted settings. Never echo them into installer logs.
        return Err(invalid_target(&format!(
            "Setup target request was refused (HTTP {}).",
            response.status()
        )));
    }
    response
        .json()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Invalid setup target response".to_string(),
        })
}

#[bon::builder]
async fn fetch_target(
    client: &reqwest::Client,
    base_url: &str,
    deployment_id: &str,
    operation_id: &str,
    release_id: &str,
) -> Result<SetupInfo> {
    let mut url = deployment_info_url(base_url, Platform::Machines, None)?;
    url.query_pairs_mut()
        .append_pair("deploymentId", deployment_id)
        .append_pair("updateOperationId", operation_id);
    let response =
        client
            .get(url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Could not load the exact pending setup target".to_string(),
            })?;
    let info: SetupInfo = read_response(response).await?;
    info.setup_update
        .validate_identity(deployment_id, operation_id, release_id)?;
    Ok(info)
}

fn manager_url(info: &SetupInfo) -> Result<&str> {
    let target = info
        .install_context
        .targets
        .get("machines")
        .filter(|target| target.platform == Platform::Machines && !target.manager_url.is_empty())
        .ok_or_else(|| {
            invalid_target("The authorized target has no Machines manager install context.")
        })?;
    Ok(&target.manager_url)
}

fn patch_settings(
    target: &mut SetupUpdateTarget,
    args: &UpArgs,
    config: Option<&DeployConfigFile>,
) -> Result<()> {
    // Only explicit choices are patched. In particular, absent settings must not
    // replace the target's update mode, telemetry, bindings, or gate answers.
    let settings = load_stack_settings(args, Platform::Machines, Platform::Machines, config)?;
    let serialized = serde_json::to_value(&settings).into_alien_error().context(
        ErrorData::ConfigurationError {
            message: "Invalid setup settings".into(),
        },
    )?;
    let mut patch = Map::new();
    if let Some(config) = config {
        for (key, supplied) in [
            ("compute", config.compute.is_some()),
            ("updates", config.updates.is_some()),
            ("telemetry", config.telemetry.is_some()),
        ] {
            if supplied {
                // Default enum choices may be omitted by StackSettings serialization.
                let value = match key {
                    "updates" => serde_json::to_value(settings.updates),
                    "telemetry" => serde_json::to_value(settings.telemetry),
                    _ => Ok(serialized[key].clone()),
                }
                .into_alien_error()
                .context(ErrorData::ConfigurationError {
                    message: "Invalid explicit setup setting".into(),
                })?;
                patch.insert(key.into(), value);
            }
        }
        if let Some(bindings) = &config.external_bindings {
            let bindings = serde_json::to_value(bindings).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Invalid external bindings".into(),
                },
            )?;
            let existing = target
                .stack_settings
                .entry("externalBindings")
                .or_insert_with(|| Value::Object(Map::new()));
            let existing = existing
                .as_object_mut()
                .ok_or_else(|| invalid_target("The target external bindings are not an object."))?;
            let bindings = bindings
                .as_object()
                .ok_or_else(|| invalid_target("External bindings must be an object."))?;
            existing.extend(bindings.clone());
        }
    }
    if let Some(network) = settings.network {
        patch.insert(
            "network".into(),
            serde_json::to_value(network).into_alien_error().context(
                ErrorData::ConfigurationError {
                    message: "Invalid network settings".into(),
                },
            )?,
        );
    }
    if let Some(endpoints) = load_public_endpoints(args, Platform::Machines, config)? {
        let existing = target
            .stack_settings
            .entry("publicEndpoints")
            .or_insert_with(|| Value::Object(Map::new()));
        let existing = existing
            .as_object_mut()
            .ok_or_else(|| invalid_target("The target public endpoints are not an object."))?;
        for (resource, endpoints) in endpoints {
            let values = existing
                .entry(resource)
                .or_insert_with(|| Value::Object(Map::new()));
            let values = values.as_object_mut().ok_or_else(|| {
                invalid_target("The target resource endpoints are not an object.")
            })?;
            values.extend(
                endpoints
                    .into_iter()
                    .map(|(name, url)| (name, Value::String(url))),
            );
        }
    }
    target.stack_settings.extend(patch);
    Ok(())
}

pub(super) async fn run(
    args: &UpArgs,
    embedded: Option<&DeployCliConfig>,
    config: Option<&DeployConfigFile>,
) -> Result<()> {
    let deployment_id = args
        .deployment_id
        .as_deref()
        .ok_or_else(|| invalid_target("--deployment-id is required."))?;
    let operation_id = args
        .update_operation_id
        .as_deref()
        .ok_or_else(|| invalid_target("--update-operation-id is required."))?;
    let release_id = args
        .release_id
        .as_deref()
        .ok_or_else(|| invalid_target("--release-id is required."))?;
    if args
        .platform
        .as_deref()
        .or_else(|| config.and_then(|config| config.platform.as_deref()))
        != Some("machines")
    {
        return Err(invalid_target(
            "Explicit setup targeting currently requires --platform machines.",
        ));
    }
    if args.base_platform.is_some()
        || config.is_some_and(|config| config.base_platform.is_some())
        || args.setup_item.is_some()
    {
        return Err(invalid_target(
            "An exact Machines setup target cannot select a base platform or setup item.",
        ));
    }
    if !args.input_values.is_empty()
        || !args.secret_input_values.is_empty()
        || config.is_some_and(|config| config.inputs.is_some() || config.secret_inputs.is_some())
    {
        return Err(invalid_target("Setup input changes must be saved through the authorized setup configuration before this command."));
    }
    let token = resolve_token(args, embedded)?;
    let base_url = resolve_base_url(args, embedded);
    let http = create_manager_http_client(&token)?;
    let mut info = fetch_target()
        .client(&http)
        .base_url(&base_url)
        .deployment_id(deployment_id)
        .operation_id(operation_id)
        .release_id(release_id)
        .call()
        .await?;
    let original_manager = manager_url(&info)?.to_string();
    if args
        .manager_url
        .as_ref()
        .is_some_and(|url| url != &original_manager)
    {
        return Err(invalid_target(
            "--manager-url does not match the existing deployment's authorized manager.",
        ));
    }
    patch_settings(&mut info.setup_update, args, config)?;
    let response = http
        .post(format!(
            "{}/v1/deployment-info/prepare-stack",
            base_url.trim_end_matches('/')
        ))
        .json(&PrepareRequest {
            deployment_id,
            update_operation_id: operation_id,
            platform: Platform::Machines,
            setup_method: "cli",
            save_for_setup: true,
            stack_settings: &info.setup_update.stack_settings,
        })
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::ConfigurationError {
            message: "Could not save the exact setup choices".into(),
        })?;
    let prepared: PreparedTarget = read_response(response).await?;
    if prepared.platform != Platform::Machines || prepared.update_operation_id.is_empty() {
        return Err(invalid_target(
            "Preparation did not return an exact Machines operation.",
        ));
    }
    output::info(&format!(
        "Setup choices saved for update operation '{}'. Use this operation ID when resuming interrupted setup.",
        prepared.update_operation_id
    ));
    let info = fetch_target()
        .client(&http)
        .base_url(&base_url)
        .deployment_id(deployment_id)
        .operation_id(&prepared.update_operation_id)
        .release_id(release_id)
        .call()
        .await?;
    if manager_url(&info)? != original_manager {
        return Err(invalid_target(
            "The original manager changed during setup preparation.",
        ));
    }
    let client = create_manager_client(&token, &original_manager)?;
    push_initial_setup_targeted()
        .client(&client)
        .deployment_id(deployment_id)
        .platform(Platform::Machines)
        .client_config(ClientConfig::Machines)
        .manager_base_url(&original_manager)
        .deployment_token(&token)
        .expected_target(&info.setup_update)
        .maybe_setup_revision(embedded.and_then(|config| config.setup_revision.as_deref()))
        .call()
        .await?;
    output::success("Setup applied. The manager will continue the requested update; workload convergence is still pending.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_deployment::manager_api_transport::ExecutionClaim;
    use clap::Parser;
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };
    use serde_json::json;

    fn target(operation: &str) -> SetupUpdateTarget {
        serde_json::from_value(json!({
            "deploymentId": "dep_demo", "updateOperationId": operation,
            "releaseId": "rel_target", "platform": "machines", "setupMethod": "cli",
            "stackSettings": {
                "updates": "approval-required", "telemetry": "off",
                "externalBindings": { "existing": { "type": "storage", "service": "s3", "bucketName": "existing-archive" } },
                "futureSetting": { "preserve": true }
            }
        })).unwrap()
    }

    fn info(server: &MockServer, operation: &str) -> Value {
        json!({
            "setupUpdate": {
                "deploymentId": "dep_demo", "updateOperationId": operation,
                "releaseId": "rel_target", "platform": "machines", "setupMethod": "cli",
                "stackSettings": {}
            },
            "installContext": { "targets": { "machines": {
                "platform": "machines", "managerUrl": server.base_url()
            } } }
        })
    }

    fn args(server: &MockServer) -> UpArgs {
        UpArgs::parse_from([
            "democtl",
            "--setup-update",
            "--deployment-id",
            "dep_demo",
            "--update-operation-id",
            "op_blocked",
            "--release-id",
            "rel_target",
            "--platform",
            "machines",
            "--token",
            "test-setup-token",
            "--base-url",
            &server.base_url(),
        ])
    }

    #[test]
    fn explicit_target_requires_all_ids_and_setup_mode() {
        for flags in [
            vec!["democtl", "--deployment-id", "dep_demo"],
            vec![
                "democtl",
                "--setup-update",
                "--deployment-id",
                "dep_demo",
                "--update-operation-id",
                "op_blocked",
            ],
            vec!["democtl", "--update-operation-id", "op_blocked"],
        ] {
            assert!(UpArgs::try_parse_from(flags).is_err());
        }
    }

    #[test]
    fn patch_preserves_unrelated_choices_and_other_external_resources() {
        let mut target = target("op_blocked");
        let config: DeployConfigFile = toml::from_str(
            r#"
            updates = "auto"
            [externalBindings.archive]
            type = "storage"
            service = "s3"
            bucketName = "customer-archive"
        "#,
        )
        .unwrap();
        patch_settings(&mut target, &UpArgs::parse_from(["democtl"]), Some(&config)).unwrap();
        assert_eq!(target.stack_settings["updates"], "auto");
        assert_eq!(target.stack_settings["telemetry"], "off");
        assert_eq!(
            target.stack_settings["futureSetting"],
            json!({"preserve": true})
        );
        assert_eq!(
            target.stack_settings["externalBindings"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            target.stack_settings["externalBindings"]["existing"]["bucketName"],
            "existing-archive"
        );
        assert_eq!(
            target.stack_settings["externalBindings"]["archive"]["bucketName"],
            "customer-archive"
        );
    }

    #[test]
    fn endpoint_patch_keeps_other_resources_and_named_endpoints() {
        let mut target = target("op_blocked");
        target.stack_settings.insert("publicEndpoints".into(), json!({
            "api": { "http": "https://old.example.com", "metrics": "https://metrics.example.com" },
            "worker": { "http": "https://worker.example.com" }
        }));
        let args = UpArgs::parse_from([
            "democtl",
            "--public-endpoint",
            "api.http=https://new.example.com",
        ]);
        patch_settings(&mut target, &args, None).unwrap();
        assert_eq!(
            target.stack_settings["publicEndpoints"],
            json!({
                "api": { "http": "https://new.example.com", "metrics": "https://metrics.example.com" },
                "worker": { "http": "https://worker.example.com" }
            })
        );
    }

    #[test]
    fn exact_claim_refuses_missing_claim_wrong_operation_deployment_or_release() {
        let target = target("op_saved");
        let valid = AcquiredDeploymentPayload {
            deployment: json!({"id": "dep_demo", "desiredReleaseId": "rel_target"}),
            execution_claim: Some(ExecutionClaim {
                operation_id: "op_saved".into(),
                attempt_id: "attempt_demo".into(),
            }),
        };
        target.validate_claim(&valid).unwrap();
        let mut invalid = valid.clone();
        invalid.execution_claim = None;
        assert!(target.validate_claim(&invalid).is_err());
        invalid = valid.clone();
        invalid.execution_claim.as_mut().unwrap().operation_id = "op_newer".into();
        assert!(target.validate_claim(&invalid).is_err());
        for (field, value) in [("id", "dep_other"), ("desiredReleaseId", "rel_newer")] {
            invalid = valid.clone();
            invalid.deployment[field] = json!(value);
            assert!(target.validate_claim(&invalid).is_err());
        }
    }

    #[tokio::test]
    async fn rejected_exact_target_never_prepares_or_initializes() {
        let server = MockServer::start_async().await;
        let get = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/deployment-info")
                    .query_param("deploymentId", "dep_demo")
                    .query_param("updateOperationId", "op_blocked");
                then.status(409).body("response body must not be logged");
            })
            .await;
        let writes = server
            .mock_async(|when, then| {
                when.method(POST);
                then.status(500);
            })
            .await;
        let error = super::super::up_command(args(&server), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("409"));
        assert!(!format!("{error:?}").contains("response body must not be logged"));
        get.assert_hits_async(1).await;
        writes.assert_hits_async(0).await;
    }

    #[tokio::test]
    async fn saved_operation_is_followed_and_stale_acquired_claim_is_released() {
        let server = MockServer::start_async().await;
        let original = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/deployment-info")
                    .query_param("updateOperationId", "op_blocked");
                then.status(200).json_body(info(&server, "op_blocked"));
            })
            .await;
        let save = server.mock_async(|when, then| {
            when.method(POST).path("/v1/deployment-info/prepare-stack").json_body(json!({
                "deploymentId": "dep_demo", "updateOperationId": "op_blocked",
                "platform": "machines", "setupMethod": "cli", "saveForSetup": true, "stackSettings": {}
            }));
            then.status(200).json_body(json!({"platform": "machines", "updateOperationId": "op_saved"}));
        }).await;
        let saved = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/deployment-info")
                    .query_param("updateOperationId", "op_saved");
                then.status(200).json_body(info(&server, "op_saved"));
            })
            .await;
        let deployment = server.mock_async(|when, then| {
            when.method(GET).path("/v1/deployments/dep_demo");
            then.status(200).json_body(json!({
                "id": "dep_demo", "name": "demo", "platform": "machines", "status": "running",
                "deploymentGroupId": "dg_demo", "deploymentProtocolVersion": 1,
                "projectId": "prj_demo", "workspaceId": "ws_demo", "retryRequested": false,
                "createdAt": "2026-01-01T00:00:00Z", "desiredReleaseId": "rel_target", "stackSettings": {}
            }));
        }).await;
        let acquire = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/acquire");
                then.status(200).json_body(json!({ "deployments": [{
                "deployment": {"id": "dep_demo", "desiredReleaseId": "rel_target"},
                "executionClaim": {"operationId": "op_newer", "attemptId": "attempt_demo"}
            }] }));
            })
            .await;
        let release = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/release");
                then.status(200);
            })
            .await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(500);
            })
            .await;
        let init = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/deployments/init");
                then.status(500);
            })
            .await;
        let error = super::super::up_command(args(&server), None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("execution claim"), "{error}");
        for mock in [&original, &save, &saved, &deployment, &acquire, &release] {
            mock.assert_hits_async(1).await;
        }
        reconcile.assert_hits_async(0).await;
        init.assert_hits_async(0).await;
    }
}
