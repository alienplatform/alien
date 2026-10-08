//! Configure an exact existing setup target before acquiring its execution claim.

use super::{
    collect_deployer_input_values, create_manager_client, create_manager_http_client,
    deployment_info_url, load_public_endpoints, load_stack_settings, push_initial_setup_targeted,
    resolve_base_url, resolve_token, stack_input_matches_context, DeployConfigFile,
    DeploymentInfoSetupConfig, SetupRunOutcome, UpArgs,
};
use crate::{
    error::{ErrorData, Result},
    output,
};
use alien_core::{
    embedded_config::DeployCliConfig, ClientConfig, ExternalBinding, ExternalBindings, Platform,
    StorageBinding,
};
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InputUpdateRequest<'a> {
    expected_base_operation_id: &'a str,
    input_values: &'a HashMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputUpdateResponse {
    outcome: String,
    operation: Option<InputOperation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputOperation {
    id: String,
    target_release_id: Option<String>,
}

impl InputUpdateResponse {
    fn next_operation(self, previous: &str, release: &str) -> Result<String> {
        match (self.outcome.as_str(), self.operation) {
            ("unchanged", None) => Ok(previous.to_string()),
            ("accepted", Some(operation))
                if !operation.id.is_empty()
                    && operation.target_release_id.as_deref() == Some(release) =>
            {
                Ok(operation.id)
            }
            _ => Err(invalid_target(
                "Input save did not return the exact requested release operation.",
            )),
        }
    }
}

fn has_explicit_inputs(args: &UpArgs, config: Option<&DeployConfigFile>) -> bool {
    !args.input_values.is_empty()
        || !args.secret_input_values.is_empty()
        || config.is_some_and(|config| {
            config
                .inputs
                .as_ref()
                .is_some_and(|values| !values.is_empty())
                || config
                    .secret_inputs
                    .as_ref()
                    .is_some_and(|values| !values.is_empty())
        })
}

fn explicit_input_values(
    args: &UpArgs,
    config: Option<&DeployConfigFile>,
    setup_config: Option<&DeploymentInfoSetupConfig>,
) -> Result<HashMap<String, Value>> {
    let supplied = has_explicit_inputs(args, config);
    if !supplied {
        return Ok(HashMap::new());
    }
    let mut inputs = setup_config
        .and_then(|config| config.inputs.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|input| stack_input_matches_context(input, Platform::Machines))
        .collect::<Vec<_>>();
    if inputs.is_empty() {
        return Err(invalid_target(
            "The exact target has no deployer input definitions for the supplied values.",
        ));
    }
    // This is a patch: required values already stored by setup must be retained,
    // not prompted for again or replaced by release defaults.
    for input in &mut inputs {
        input.required = false;
    }
    collect_deployer_input_values(
        &inputs,
        &args.input_values,
        &args.secret_input_values,
        config,
    )
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

// Signing credentials travel through encrypted Secret inputs into workload
// environment delivery; Machines setup bindings contain only store locators.
pub(super) fn validate_machines_binding_credentials(bindings: &ExternalBindings) -> Result<()> {
    for binding in bindings.0.values() {
        if let ExternalBinding::Storage(StorageBinding::S3(storage)) = binding {
            if storage.access_key_id.is_some() || storage.secret_access_key.is_some() {
                return Err(AlienError::new(ErrorData::ValidationError {
                    field: "externalBindings".into(),
                    message: "Machines setup bindings must omit accessKeyId and secretAccessKey. Supply encrypted Secret inputs mapped to AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY instead.".into(),
                }));
            }
        }
    }
    Ok(())
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
            for (resource, supplied) in bindings {
                let supplied = supplied
                    .as_object()
                    .ok_or_else(|| invalid_target("Each external binding must be an object."))?;
                let saved = existing
                    .get_mut(resource)
                    .and_then(Value::as_object_mut)
                    .filter(|saved| {
                        saved.get("type") == supplied.get("type")
                            && saved.get("service") == supplied.get("service")
                    });
                if let Some(saved) = saved {
                    // Omitted fields retain the exact saved locator and credential
                    // references. A different type or service replaces the binding.
                    saved.extend(supplied.clone());
                } else {
                    existing.insert(resource.clone(), Value::Object(supplied.clone()));
                }
            }
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
    if let Some(bindings) = target.stack_settings.get("externalBindings") {
        let bindings = serde_json::from_value::<ExternalBindings>(bindings.clone())
            .map_err(|_| invalid_target("Invalid saved external bindings."))?;
        validate_machines_binding_credentials(&bindings)?;
    }
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
    let mut operation_id = args
        .update_operation_id
        .as_deref()
        .ok_or_else(|| invalid_target("--update-operation-id is required."))?
        .to_string();
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
    let token = resolve_token(args, embedded)?;
    let base_url = resolve_base_url(args, embedded);
    let http = create_manager_http_client(&token)?;
    let mut info = fetch_target()
        .client(&http)
        .base_url(&base_url)
        .deployment_id(deployment_id)
        .operation_id(&operation_id)
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
    if has_explicit_inputs(args, config) {
        let inputs_url = format!(
            "{}/v1/deployments/{}/inputs",
            base_url.trim_end_matches('/'),
            urlencoding::encode(deployment_id)
        );
        let response = http
            .get(&inputs_url)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Could not load setup input definitions".into(),
            })?;
        let definitions: DeploymentInfoSetupConfig = read_response(response).await?;
        let values = explicit_input_values(args, config, Some(&definitions))?;
        let response = http
            .patch(&inputs_url)
            .json(&InputUpdateRequest {
                expected_base_operation_id: &operation_id,
                input_values: &values,
            })
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "Could not save the exact setup inputs".into(),
            })?;
        let saved: InputUpdateResponse = read_response(response).await?;
        operation_id = saved.next_operation(&operation_id, release_id)?;
        output::info(&format!("Setup inputs saved for update operation '{operation_id}'. Use this operation ID if setup is interrupted."));
        info = fetch_target()
            .client(&http)
            .base_url(&base_url)
            .deployment_id(deployment_id)
            .operation_id(&operation_id)
            .release_id(release_id)
            .call()
            .await?;
        if manager_url(&info)? != original_manager {
            return Err(invalid_target(
                "The original manager changed while saving setup inputs.",
            ));
        }
    }
    patch_settings(&mut info.setup_update, args, config)?;
    let response = http
        .post(format!(
            "{}/v1/deployment-info/prepare-stack",
            base_url.trim_end_matches('/')
        ))
        .json(&PrepareRequest {
            deployment_id,
            update_operation_id: &operation_id,
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
    let outcome = push_initial_setup_targeted()
        .client(&client)
        .deployment_id(deployment_id)
        .platform(Platform::Machines)
        .client_config(ClientConfig::Machines)
        .manager_base_url(&original_manager)
        .deployment_token(&token)
        .expected_target(&info.setup_update)
        .network_args(&args.network)
        .maybe_setup_revision(embedded.and_then(|config| config.setup_revision.as_deref()))
        .call()
        .await?;
    if outcome == SetupRunOutcome::Applied {
        output::success("Setup applied. The manager will continue the requested update; workload convergence is still pending.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
        ResourceLifecycle, RuntimeMetadata, Stack, StackInputDefinition, StackInputKind,
        StackSettings, StackState, Storage, StorageBinding,
    };
    use alien_deployment::manager_api_transport::ExecutionClaim;
    use clap::Parser;
    use httpmock::{
        Method::{GET, PATCH, POST},
        MockServer,
    };
    use serde_json::json;
    use std::io::Write;

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

    fn input_definitions() -> Value {
        let mut secret = StackInputDefinition::deployer_boolean(
            "storageSecret",
            "Storage secret",
            "Signing secret",
            None,
        );
        secret.kind = StackInputKind::Secret;
        json!({"inputs": [
            secret,
            StackInputDefinition::deployer_boolean("archiveEnabled", "Archive", "Enable archive", None),
            StackInputDefinition::deployer_boolean("alreadyConfigured", "Existing choice", "Keep prior answer", None)
        ]})
    }

    #[test]
    fn input_patch_only_contains_explicit_typed_values() {
        let args = UpArgs::parse_from(["democtl", "--input", "archiveEnabled=true"]);
        let definitions: DeploymentInfoSetupConfig =
            serde_json::from_value(input_definitions()).unwrap();
        let values = explicit_input_values(&args, None, Some(&definitions)).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values["archiveEnabled"], true);
        assert!(!values.contains_key("storageSecret"));
        assert!(!values.contains_key("alreadyConfigured"));
        let malformed = UpArgs::parse_from(["democtl", "--secret-input", "sensitive-test-marker"]);
        let error = explicit_input_values(&malformed, None, Some(&definitions)).unwrap_err();
        assert!(!format!("{error:?}").contains("sensitive-test-marker"));
    }

    #[test]
    fn exact_input_patch_refuses_deployer_secret_values() {
        let definitions: DeploymentInfoSetupConfig =
            serde_json::from_value(input_definitions()).unwrap();
        for flag in ["--input", "--secret-input"] {
            let args = UpArgs::parse_from(["democtl", flag, "storageSecret=canary-secret"]);
            let error = explicit_input_values(&args, None, Some(&definitions)).unwrap_err();
            assert!(error.to_string().contains("Storage secret"));
            assert!(!format!("{error:?}").contains("canary-secret"));
        }
        let config: DeployConfigFile = toml::from_str(
            r#"[secretInputs]
storageSecret = "canary-secret"
"#,
        )
        .unwrap();
        let error = explicit_input_values(
            &UpArgs::parse_from(["democtl"]),
            Some(&config),
            Some(&definitions),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Storage secret"));
        assert!(!format!("{error:?}").contains("canary-secret"));
    }

    #[test]
    fn input_save_must_preserve_exact_release_and_return_an_operation() {
        for response in [
            json!({"outcome":"accepted", "operation":null}),
            json!({"outcome":"accepted", "operation":{"id":"op_other", "targetReleaseId":"rel_other"}}),
            json!({"outcome":"unexpected", "operation":null}),
        ] {
            let response: InputUpdateResponse = serde_json::from_value(response).unwrap();
            assert!(response.next_operation("op_blocked", "rel_target").is_err());
        }
    }

    #[tokio::test]
    async fn input_save_uses_cas_and_follows_accepted_or_unchanged_operation() {
        for outcome in ["accepted", "unchanged", "stale"] {
            let server = MockServer::start_async().await;
            let original = server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path("/v1/deployment-info")
                        .query_param("updateOperationId", "op_blocked");
                    then.status(200).json_body(info(&server, "op_blocked"));
                })
                .await;
            let definitions = server
                .mock_async(|when, then| {
                    when.method(GET).path("/v1/deployments/dep_demo/inputs");
                    then.status(200).json_body(input_definitions());
                })
                .await;
            let inputs = server.mock_async(|when, then| {
                when.method(PATCH).path("/v1/deployments/dep_demo/inputs").json_body(json!({
                    "expectedBaseOperationId":"op_blocked",
                    "inputValues":{"archiveEnabled":true}
                }));
                if outcome == "stale" { then.status(409).body("test-secret"); }
                else { then.status(200).json_body(json!({
                    "outcome":outcome,
                    "operation": if outcome == "accepted" { json!({"id":"op_inputs","targetReleaseId":"rel_target"}) } else { Value::Null }
                })); }
            }).await;
            let saved = server
                .mock_async(|when, then| {
                    when.method(GET)
                        .path("/v1/deployment-info")
                        .query_param("updateOperationId", "op_inputs");
                    then.status(200).json_body(info(&server, "op_inputs"));
                })
                .await;
            let prepare = server.mock_async(|when, then| {
                when.method(POST).path("/v1/deployment-info/prepare-stack").json_body(json!({
                    "deploymentId":"dep_demo", "updateOperationId":if outcome=="accepted" {"op_inputs"} else {"op_blocked"},
                    "platform":"machines", "setupMethod":"cli", "saveForSetup":true, "stackSettings":{}
                }));
                then.status(409);
            }).await;
            let acquire = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/sync/acquire");
                    then.status(500);
                })
                .await;
            let mut args = args(&server);
            args.input_values = vec!["archiveEnabled=true".into()];
            let error = super::super::up_command(args, None).await.unwrap_err();
            assert!(error.to_string().contains("409"), "{error}");
            assert!(!format!("{error:?}").contains("test-secret"));
            original
                .assert_hits_async(if outcome == "unchanged" { 2 } else { 1 })
                .await;
            definitions.assert_hits_async(1).await;
            inputs.assert_hits_async(1).await;
            saved
                .assert_hits_async(if outcome == "accepted" { 1 } else { 0 })
                .await;
            prepare
                .assert_hits_async(if outcome == "stale" { 0 } else { 1 })
                .await;
            acquire.assert_hits_async(0).await;
        }
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

    #[tokio::test]
    async fn binding_patch_preserves_saved_fields_in_prepare_request() {
        let server = MockServer::start_async().await;
        let binding = json!({
            "type": "storage", "service": "s3", "bucketName": "old-archive",
            "endpoint": "https://objects.example.com", "region": "us-east-1",
            "forcePathStyle": true
        });
        let mut expected = binding.clone();
        expected["bucketName"] = json!("new-archive");
        let mut response = info(&server, "op_blocked");
        response["setupUpdate"]["stackSettings"] =
            json!({"externalBindings": {"archive": binding}});
        server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/deployment-info");
                then.status(200).json_body(response);
            })
            .await;
        let prepare = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/deployment-info/prepare-stack")
                    .json_body(json!({
                        "deploymentId": "dep_demo", "updateOperationId": "op_blocked",
                        "platform": "machines", "setupMethod": "cli", "saveForSetup": true,
                        "stackSettings": {"externalBindings": {"archive": expected}}
                    }));
                // Stop after capturing the real outgoing request, before acquisition.
                then.status(503);
            })
            .await;
        let config: DeployConfigFile = serde_json::from_value(json!({"externalBindings": {
            "archive": {"type": "storage", "service": "s3", "bucketName": "new-archive"}
        }}))
        .unwrap();
        assert!(run(&args(&server), None, Some(&config)).await.is_err());
        prepare.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn binding_secret_values_are_refused_before_prepare_request() {
        for from_saved_target in [false, true] {
            let server = MockServer::start_async().await;
            let binding = json!({"type": "storage", "service": "s3", "bucketName": "archive",
                "secretAccessKey": "sensitive-signing-marker"});
            let mut response = info(&server, "op_blocked");
            if from_saved_target {
                response["setupUpdate"]["stackSettings"] =
                    json!({"externalBindings": {"archive": binding.clone()}});
            }
            server
                .mock_async(|when, then| {
                    when.method(GET).path("/v1/deployment-info");
                    then.status(200).json_body(response);
                })
                .await;
            let prepare = server
                .mock_async(|when, then| {
                    when.method(POST).path("/v1/deployment-info/prepare-stack");
                    then.status(503);
                })
                .await;
            let config: DeployConfigFile = serde_json::from_value(json!({"externalBindings": {
                "archive": if from_saved_target {
                    json!({"type": "storage", "service": "s3", "bucketName": "new-archive"})
                } else { binding }
            }}))
            .unwrap();
            let error = run(&args(&server), None, Some(&config)).await.unwrap_err();
            prepare.assert_hits_async(0).await;
            assert!(error.to_string().contains("encrypted Secret inputs"));
            assert!(!format!("{error:?}").contains("sensitive-signing-marker"));
        }
    }

    #[test]
    fn binding_service_change_replaces_incompatible_saved_fields() {
        let mut target = target("op_blocked");
        target.stack_settings["externalBindings"]["existing"]["endpoint"] =
            json!("https://objects.example.com");
        let config: DeployConfigFile = serde_json::from_value(json!({"externalBindings": {
            "existing": {"type": "storage", "service": "gcs", "bucketName": "archive"}
        }}))
        .unwrap();
        patch_settings(&mut target, &UpArgs::parse_from(["democtl"]), Some(&config)).unwrap();
        assert_eq!(
            target.stack_settings["externalBindings"]["existing"],
            json!({"type": "storage", "service": "gcs", "bucketName": "archive"})
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
    async fn fresh_runner_uses_acquired_bindings_and_hands_off_at_provisioning() {
        run_fresh_runner(false).await;
    }

    #[tokio::test]
    async fn fresh_runner_rejects_endpoint_access_change_under_claim() {
        run_fresh_runner(true).await;
    }

    async fn run_fresh_runner(change_endpoint_access: bool) {
        let server = MockServer::start_async().await;
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "archive",
            ExternalBinding::Storage(StorageBinding::s3("customer-archive")),
        );
        let settings = StackSettings {
            external_bindings: Some(bindings.clone()),
            ..Default::default()
        };
        let mut config = DeploymentConfig::builder()
            .allow_frozen_changes(false)
            .stack_settings(settings.clone())
            .external_bindings(bindings)
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .build();
        config.deployment_token = Some("test-runtime-token".into());
        let old_stack = Stack::new("demo".into()).build();
        let target_stack = Stack::new("demo".into())
            .add(
                Storage::new("archive".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let original = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/deployment-info")
                    .query_param("updateOperationId", "op_blocked");
                let mut value = info(&server, "op_blocked");
                value["setupUpdate"]["stackSettings"] = json!({"endpointAccess": "internet"});
                then.status(200).json_body(value);
            })
            .await;
        let save = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/deployment-info/prepare-stack").json_body(json!({
                "deploymentId":"dep_demo", "updateOperationId":"op_blocked", "platform":"machines",
                "setupMethod":"cli", "saveForSetup":true, "stackSettings":settings,
            }));
                then.status(200)
                    .json_body(json!({"platform":"machines", "updateOperationId":"op_saved"}));
            })
            .await;
        let saved = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/v1/deployment-info")
                    .query_param("updateOperationId", "op_saved");
                let mut value = info(&server, "op_saved");
                value["setupUpdate"]["stackSettings"] = serde_json::to_value(&settings).unwrap();
                then.status(200).json_body(value);
            })
            .await;
        let deployment = server.mock_async(|when, then| {
            when.method(GET).path("/v1/deployments/dep_demo");
            then.status(200).json_body(json!({
                "id":"dep_demo", "name":"demo", "platform":"machines", "status":"running",
                "deploymentGroupId":"dg_demo", "deploymentProtocolVersion":1,
                "projectId":"prj_demo", "workspaceId":"ws_demo", "retryRequested":false,
                "createdAt":"2026-01-01T00:00:00Z", "currentReleaseId":"rel_installed", "desiredReleaseId":"rel_target",
                "stackSettings":{}, "stackState":StackState::new(Platform::Machines),
                "runtimeMetadata":RuntimeMetadata { prepared_stack:Some(old_stack.clone()), ..Default::default() }
            }));
        }).await;
        let acquire = server.mock_async(|when, then| {
            when.method(POST).path("/v1/sync/acquire")
                .json_body_partial(json!({"deploymentIds":["dep_demo"],"acquireMode":"setup-run","setupMethod":"cli"}).to_string());
            then.status(200).json_body(json!({"deployments":[{
                "deployment":{"id":"dep_demo","desiredReleaseId":"rel_target","deploymentConfig":config},
                "executionClaim":{"operationId":"op_saved","attemptId":"attempt_demo"}
            }]}));
        }).await;
        let mut releases = Vec::new();
        for (id, stack) in [("rel_installed", old_stack), ("rel_target", target_stack)] {
            releases.push(server.mock_async(|when, then| {
                when.method(GET).path(format!("/v1/releases/{id}"));
                then.status(200).json_body(json!({
                    "id":id,"workspaceId":"ws_demo","projectId":"prj_demo",
                    "stack":{"machines":stack},"createdAt":"2026-01-01T00:00:00Z","setupFingerprints":{}
                }));
            }).await);
        }
        let _renew = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/renew");
                then.status(200);
            })
            .await;
        let reconcile = server.mock_async(|when, then| {
            when.method(POST).path("/v1/sync/reconcile").json_body_partial(json!({
                "deploymentId":"dep_demo", "executionClaim":{"operationId":"op_saved","attemptId":"attempt_demo"},
                "state":{"status":"provisioning", "currentRelease":{"releaseId":"rel_installed"},
                    "targetRelease":{"releaseId":"rel_target"}, "stackState":{"resources":{"archive":{"status":"running"}}}}
            }).to_string());
            then.status(200).json_body(json!({"success":true,"current":null}));
        }).await;
        let release = server.mock_async(|when, then| {
            when.method(POST).path("/v1/sync/release").json_body_partial(json!({
                "deploymentId":"dep_demo", "executionClaim":{"operationId":"op_saved","attemptId":"attempt_demo"}
            }).to_string()); then.status(200);
        }).await;
        let init = server
            .mock_async(|when, then| {
                when.method(POST).path("/v1/deployments/init");
                then.status(500);
            })
            .await;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "[externalBindings.archive]\ntype = \"storage\"\nservice = \"s3\"\nbucketName = \"customer-archive\"\n").unwrap();
        let mut args = args(&server);
        args.config = Some(file.path().to_path_buf());
        if change_endpoint_access {
            args.network.endpoint_access = Some(alien_core::EndpointAccess::Private);
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            super::super::up_command(args, None),
        )
        .await
        .expect("setup handoff should finish promptly");
        if change_endpoint_access {
            let error = result.expect_err("existing endpoint access must remain fixed");
            assert!(
                error
                    .to_string()
                    .contains("Endpoint access cannot change after setup"),
                "{error}"
            );
        } else {
            result.expect("exact setup should hand off to manager");
        }
        for mock in [&original, &save, &saved, &acquire, &release] {
            mock.assert_hits_async(1).await;
        }
        deployment.assert_hits_async(2).await;
        for release in releases {
            release
                .assert_hits_async(if change_endpoint_access { 0 } else { 1 })
                .await;
        }
        reconcile
            .assert_hits_async(if change_endpoint_access { 0 } else { 2 })
            .await;
        init.assert_hits_async(0).await;
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
