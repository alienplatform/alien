//! A resource type and a controller that reads what a handler adds to Alien's context, run in
//! the step protocol's host as `run_controllers` runs it, with no network. A test crate that
//! uses it imports `controller_prelude::*` at its root, where `#[controller]` looks.
#![allow(dead_code)]

use alien_core::{DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, StackSettings};
use alien_extension_sdk::alien_resource;
use alien_extension_sdk::controller_prelude::*;
use alien_infra::step_protocol::host::{step_client_config, ControllerHost};
use alien_infra::step_protocol::protocol::{ResponseBody, StepRequest, StepResponse};
use alien_infra::DefaultPlatformServiceProvider;
use aws_credential_types::provider::ProvideCredentials;
use serde_json::{json, Value};
use std::sync::Arc;

#[alien_resource(type = "probe/thing", outputs = ThingOutputs, lifecycles = [Live], holds_data = false)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema, bon::Builder)]
#[serde(rename_all = "camelCase")]
#[builder(start_fn = new)]
pub struct Thing {
    #[builder(start_fn)]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThingOutputs {
    pub name: String,
}

/// What a bucket this resource depends on outputs, read as the controller's own type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketOutputs {
    pub bucket_name: String,
}

/// Waits `pause_ms`, reads the AWS SDK config, then the outputs of `dependency` when it names
/// one.
#[controller]
pub struct ProbeController {
    dependency: Option<String>,
    pause_ms: Option<u64>,
    region: Option<String>,
    access_key_id: Option<String>,
    session_token: Option<String>,
    endpoint: Option<String>,
    bucket_name: Option<String>,
}

#[controller]
impl ProbeController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if let Some(pause_ms) = self.pause_ms {
            tokio::time::sleep(Duration::from_millis(pause_ms)).await;
        }
        let sdk_config = ctx.aws_sdk_config().await?;
        let sts = aws_sdk_sts::Client::new(&sdk_config);
        self.region = sts.config().region().map(|region| region.to_string());
        self.endpoint = sdk_config.endpoint_url().map(str::to_string);
        let credentials = sdk_config
            .credentials_provider()
            .expect("the config carries the step's credentials")
            .provide_credentials()
            .await
            .expect("static credentials resolve with no network");
        self.access_key_id = Some(credentials.access_key_id().to_string());
        self.session_token = credentials.session_token().map(str::to_string);
        if let Some(dependency) = self.dependency.clone() {
            self.bucket_name = Some(
                ctx.dependency_outputs::<BucketOutputs>(&dependency)?
                    .bucket_name,
            );
        }
        Ok(HandlerAction::Continue {
            state: Done,
            suggested_delay: None,
        })
    }

    #[flow_entry(Update)]
    #[handler(state = UpdateStart, on_failure = CreateFailed, status = ResourceStatus::Updating)]
    async fn update_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Done,
            suggested_delay: None,
        })
    }

    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = CreateFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    terminal_state!(state = Done, status = ResourceStatus::Running);
    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        None
    }
}

/// A host running `ProbeController` on AWS, reading the outputs of `dependency` if given.
pub fn host(dependency: Option<&str>) -> ControllerHost {
    host_with(ProbeController {
        dependency: dependency.map(str::to_string),
        ..Default::default()
    })
}

/// A host whose controller waits `pause_ms` before it does anything.
pub fn slow_host(pause_ms: u64) -> ControllerHost {
    host_with(ProbeController {
        pause_ms: Some(pause_ms),
        ..Default::default()
    })
}

fn host_with(controller: ProbeController) -> ControllerHost {
    let mut host = ControllerHost::new(
        step_client_config,
        Arc::new(DefaultPlatformServiceProvider::default()),
        None,
    );
    host.register::<Thing, _>(
        Thing::RESOURCE_TYPE.resource_type(),
        Platform::Aws,
        controller,
    )
    .unwrap();
    host
}

pub fn request(credentials: Value, dependencies: Value, deadline: &str) -> StepRequest {
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep_probe",
        "resourceId": "thing", "resourceType": "probe/thing", "platform": "aws",
        "resourcePrefix": "q6aa0b26", "initialSetupAuthority": "importedHandoff",
        "lifecycle": "live", "resource": { "type": "probe/thing", "id": "thing" },
        "previousResource": null, "dependencies": dependencies,
        "deploymentConfig": serde_json::to_value(config).unwrap(),
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": credentials, "deadline": deadline
    }))
    .unwrap()
}

pub const FAR_DEADLINE: &str = "2099-01-01T00:00:00Z";

pub fn aws_credentials() -> Value {
    json!({
        "platform": "aws", "accountId": "123456789012", "region": "eu-west-3",
        "accessKeyId": "ASIASTEPKEY", "secretAccessKey": "step-secret",
        "sessionToken": "step-session", "expiresAt": "2099-01-01T00:00:00Z"
    })
}

pub fn bucket() -> Value {
    json!({
        "bucket": {
            "resourceType": "storage", "status": "running",
            "outputs": { "type": "storage", "bucketName": "stack-bucket" },
            "bindingParams": null
        }
    })
}

/// The controller's saved state after a step that ran.
pub fn stepped(reply: StepResponse) -> Value {
    match reply.body {
        ResponseBody::Stepped { controller, .. } => controller.state,
        other => panic!("expected a step, got {other:?}"),
    }
}

/// The error of a step whose handler failed: code, message and whether it is retryable.
pub fn failed(reply: StepResponse) -> (String, String, bool) {
    match reply.body {
        ResponseBody::Failed { error, .. } => (error.code, error.message, error.retryable),
        other => panic!("expected a failed step, got {other:?}"),
    }
}

/// The error of a step the host refused before running it.
pub fn refused(reply: StepResponse) -> (String, String) {
    match reply.body {
        ResponseBody::Refused { error, .. } => (error.code, error.message),
        other => panic!("expected a refusal, got {other:?}"),
    }
}
