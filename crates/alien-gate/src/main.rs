//! The gate as a Lambda function.
//!
//! Setup creates it with this configuration, which only the gate's own role
//! can change:
//!
//! - `ALIEN_GATE_ANCHOR`: the deployment's anchor, as JSON;
//! - `ALIEN_GATE_FUNCTION_PREFIX`: the functions it owns are `<prefix><workload>`;
//! - `ALIEN_GATE_REGISTRY`: repository prefix relocated images must start with;
//! - `ALIEN_GATE_ROLES`: `{"<permission profile>": "<role ARN>"}`;
//! - `ALIEN_GATE_STATE_PARAMETER`: SSM parameter holding the gate's state.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

use std::collections::BTreeMap;

use alien_aws_clients::lambda::LambdaClient;
use alien_aws_clients::ssm::{GetParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gate::{
    apply, check_read_back, plan, Action, ErrorData, FunctionStore, GateConfig, GateRequest,
    GateState, Result,
};
use alien_release_trust::Anchor;
use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::{json, Value};
use tracing::info;

/// Anchor `inputs` prefix naming an SSM parameter.
const SSM_INPUTS_PREFIX: &str = "aws-ssm:";

struct Gate {
    config: GateConfig,
    state_parameter: String,
    lambda: LambdaClient,
    ssm: SsmClient,
}

fn env(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("{name} is not set"),
        })
    })
}

impl Gate {
    async fn from_env() -> Result<Self> {
        let anchor = Anchor::parse(env("ALIEN_GATE_ANCHOR")?.as_bytes()).context(
            ErrorData::ConfigurationInvalid {
                message: "ALIEN_GATE_ANCHOR is not a valid anchor".to_string(),
            },
        )?;
        let roles: BTreeMap<String, String> = serde_json::from_str(&env("ALIEN_GATE_ROLES")?)
            .into_alien_error()
            .context(ErrorData::ConfigurationInvalid {
                message: "ALIEN_GATE_ROLES is not a map of role ARNs".to_string(),
            })?;
        let aws =
            AwsClientConfig::from_std_env()
                .await
                .context(ErrorData::ConfigurationInvalid {
                    message: "AWS credentials are not available".to_string(),
                })?;
        let credentials = AwsCredentialProvider::from_config(aws).await.context(
            ErrorData::ConfigurationInvalid {
                message: "AWS credentials are not available".to_string(),
            },
        )?;
        let http = reqwest::Client::new();
        Ok(Self {
            config: GateConfig {
                anchor,
                function_prefix: env("ALIEN_GATE_FUNCTION_PREFIX")?,
                registry: env("ALIEN_GATE_REGISTRY")?,
                roles,
            },
            state_parameter: env("ALIEN_GATE_STATE_PARAMETER")?,
            lambda: LambdaClient::new(http.clone(), credentials.clone()),
            ssm: SsmClient::new(http, credentials),
        })
    }

    /// Deployer inputs from the customer's own parameter, if the anchor names one.
    async fn deployer_inputs(&self) -> Result<BTreeMap<String, String>> {
        let Some(name) = self
            .config
            .anchor
            .inputs
            .as_deref()
            .and_then(|inputs| inputs.strip_prefix(SSM_INPUTS_PREFIX))
        else {
            return Ok(BTreeMap::new());
        };
        let response = self
            .ssm
            .get_parameter(GetParameterRequest {
                name: name.to_string(),
                with_decryption: Some(true),
            })
            .await;
        let value = match response {
            Ok(response) => response.parameter.and_then(|parameter| parameter.value),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                None
            }
            Err(error) => {
                return Err(error).context(ErrorData::CloudCallFailed {
                    operation: format!("ssm:GetParameter {name}"),
                })
            }
        };
        match value {
            None => Ok(BTreeMap::new()),
            Some(text) => serde_json::from_str(&text).into_alien_error().context(
                ErrorData::ConfigurationInvalid {
                    message: format!("parameter {name} is not a JSON object of strings"),
                },
            ),
        }
    }

    async fn handle(&self, event: Value) -> Result<Value> {
        let request: GateRequest =
            serde_json::from_value(event)
                .into_alien_error()
                .context(ErrorData::Refused {
                    reason: "the request is not one the gate accepts".to_string(),
                })?;
        let state = GateState::load(&self.ssm, &self.state_parameter).await?;
        let inputs = self.deployer_inputs().await?;
        let planned = plan(&self.config, &state, &request, &inputs, chrono::Utc::now())?;
        let function = &planned.function;
        match request.action {
            Action::Apply => {
                apply(&self.lambda, function).await?;
                planned
                    .state
                    .store(&self.ssm, &self.state_parameter)
                    .await?;
                info!(function = %function.name, digest = %function.digest, "applied the signed function");
            }
            Action::Check => {
                let back = FunctionStore::get(&self.lambda, &function.name).await?;
                check_read_back(function, &back)?;
            }
        }
        Ok(json!({
            "function": function.name,
            "imageUri": function.image_uri,
            "digest": function.digest,
        }))
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), lambda_runtime::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();
    let gate = Gate::from_env()
        .await
        .map_err(|error| lambda_runtime::Error::from(error.to_string()))?;
    let gate = &gate;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        gate.handle(event.payload).await.map_err(|error| {
            // The whole chain: the outer reason and what caused it.
            let causes =
                std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
                    .map(|cause| cause.message.clone());
            let message = std::iter::once(error.message.clone())
                .chain(causes)
                .collect::<Vec<_>>()
                .join(": ");
            lambda_runtime::Error::from(message)
        })
    }))
    .await
}
