//! The gate as a Lambda function on AWS, a Cloud Run service on GCP or a
//! Container App on Azure.
//!
//! Setup creates it with this configuration, which only the gate's own
//! identity can change:
//!
//! - `ALIEN_GATE_CLOUD`: `aws` (the default), `gcp` or `azure`;
//! - `ALIEN_GATE_ANCHOR`: the deployment's anchor, as JSON;
//! - `ALIEN_GATE_FUNCTION_PREFIX`: the functions it owns are `<prefix><workload>`;
//! - `ALIEN_GATE_REGISTRY`: repository prefix relocated images must start with;
//! - `ALIEN_GATE_ROLES`: `{"<permission profile>": "<what it runs as>"}`, see
//!   `GateConfig::roles` in the library;
//! - on AWS, `ALIEN_GATE_STATE_PARAMETER`: SSM parameter holding the gate's state;
//! - on GCP, `ALIEN_GATE_STATE_SECRET`: Secret Manager secret holding it, and
//!   `ALIEN_GATE_GCP_PROJECT` and `ALIEN_GATE_GCP_REGION`;
//! - on Azure, `ALIEN_GATE_STATE_SECRET`: Key Vault secret holding it,
//!   `ALIEN_GATE_AZURE_RESOURCE_GROUP` (`/subscriptions/<id>/resourceGroups/<name>`),
//!   `ALIEN_GATE_AZURE_VAULT` (`https://<vault>.vault.azure.net`) and, when the
//!   gate has several identities, `ALIEN_GATE_AZURE_CLIENT_ID`.
//!
//! On GCP and Azure the gate serves `POST /` on `$PORT`; the platform's own
//! authentication decides who may call it.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_aws_clients::lambda::LambdaClient;
use alien_aws_clients::ssm::{GetParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gate::azure::Azure;
use alien_gate::gcp::{Gcp, TokenSource};
use alien_gate::{
    cloud_run, container_apps, plan, Action, ErrorData, FunctionStore, GateConfig, GateRequest,
    GateState, Result,
};
use alien_release_trust::Anchor;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::{json, Value};
use tracing::info;

/// Anchor `inputs` prefix naming an SSM parameter.
const SSM_INPUTS_PREFIX: &str = "aws-ssm:";
/// Anchor `inputs` prefix naming a Secret Manager secret.
const GCP_SECRET_INPUTS_PREFIX: &str = "gcp-secret:";
/// Anchor `inputs` prefix naming a Key Vault secret.
const AZURE_SECRET_INPUTS_PREFIX: &str = "azure-keyvault:";

enum Cloud {
    Aws {
        state_parameter: String,
        lambda: LambdaClient,
        ssm: SsmClient,
    },
    Gcp {
        state_secret: String,
        gcp: Gcp,
    },
    Azure {
        state_secret: String,
        azure: Azure,
    },
}

struct Gate {
    config: GateConfig,
    cloud: Cloud,
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
                message: "ALIEN_GATE_ROLES is not a map of identities".to_string(),
            })?;
        let config = GateConfig {
            anchor,
            function_prefix: env("ALIEN_GATE_FUNCTION_PREFIX")?,
            registry: env("ALIEN_GATE_REGISTRY")?,
            roles,
        };
        let cloud = match std::env::var("ALIEN_GATE_CLOUD").as_deref() {
            Ok("gcp") => Cloud::Gcp {
                state_secret: env("ALIEN_GATE_STATE_SECRET")?,
                gcp: Gcp::new(
                    match std::env::var("ALIEN_GATE_GCP_TOKEN_FILE") {
                        Ok(path) => TokenSource::File(path),
                        Err(_) => TokenSource::Metadata,
                    },
                    env("ALIEN_GATE_GCP_PROJECT")?,
                    env("ALIEN_GATE_GCP_REGION")?,
                ),
            },
            Ok("azure") => Cloud::Azure {
                state_secret: env("ALIEN_GATE_STATE_SECRET")?,
                azure: Azure::new(
                    env("ALIEN_GATE_AZURE_RESOURCE_GROUP")?,
                    env("ALIEN_GATE_AZURE_VAULT")?,
                    std::env::var("ALIEN_GATE_AZURE_CLIENT_ID").ok(),
                ),
            },
            Ok("aws") | Err(_) => {
                let aws = AwsClientConfig::from_std_env().await.context(
                    ErrorData::ConfigurationInvalid {
                        message: "AWS credentials are not available".to_string(),
                    },
                )?;
                let credentials = AwsCredentialProvider::from_config(aws).await.context(
                    ErrorData::ConfigurationInvalid {
                        message: "AWS credentials are not available".to_string(),
                    },
                )?;
                let http = reqwest::Client::new();
                Cloud::Aws {
                    state_parameter: env("ALIEN_GATE_STATE_PARAMETER")?,
                    lambda: LambdaClient::new(http.clone(), credentials.clone()),
                    ssm: SsmClient::new(http, credentials),
                }
            }
            Ok(other) => {
                return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                    message: format!("ALIEN_GATE_CLOUD '{other}' is not aws, gcp or azure"),
                }))
            }
        };
        Ok(Self { config, cloud })
    }

    async fn load_state(&self) -> Result<GateState> {
        match &self.cloud {
            Cloud::Aws {
                state_parameter,
                ssm,
                ..
            } => GateState::load(ssm, state_parameter).await,
            Cloud::Gcp { state_secret, gcp } => gcp.load_state(state_secret).await,
            Cloud::Azure {
                state_secret,
                azure,
            } => azure.load_state(state_secret).await,
        }
    }

    async fn store_state(&self, state: &GateState) -> Result<()> {
        match &self.cloud {
            Cloud::Aws {
                state_parameter,
                ssm,
                ..
            } => state.store(ssm, state_parameter).await,
            Cloud::Gcp { state_secret, gcp } => gcp.store_state(state_secret, state).await,
            Cloud::Azure {
                state_secret,
                azure,
            } => azure.store_state(state_secret, state).await,
        }
    }

    /// Deployer inputs from the customer's own store, if the anchor names one.
    async fn deployer_inputs(&self) -> Result<BTreeMap<String, String>> {
        let inputs = self.config.anchor.inputs.as_deref().unwrap_or_default();
        match &self.cloud {
            Cloud::Aws { ssm, .. } => match inputs.strip_prefix(SSM_INPUTS_PREFIX) {
                Some(name) => ssm_inputs(ssm, name).await,
                None => Ok(BTreeMap::new()),
            },
            Cloud::Gcp { gcp, .. } => match inputs.strip_prefix(GCP_SECRET_INPUTS_PREFIX) {
                Some(secret) => gcp.deployer_inputs(secret).await,
                None => Ok(BTreeMap::new()),
            },
            Cloud::Azure { azure, .. } => match inputs.strip_prefix(AZURE_SECRET_INPUTS_PREFIX) {
                Some(secret) => azure.deployer_inputs(secret).await,
                None => Ok(BTreeMap::new()),
            },
        }
    }

    async fn handle(&self, event: Value) -> Result<Value> {
        let request: GateRequest =
            serde_json::from_value(event)
                .into_alien_error()
                .context(ErrorData::Refused {
                    reason: "the request is not one the gate accepts".to_string(),
                })?;
        let state = self.load_state().await?;
        let inputs = self.deployer_inputs().await?;
        let planned = plan(&self.config, &state, &request, &inputs, chrono::Utc::now())?;
        let function = &planned.function;
        match (&self.cloud, request.action) {
            (Cloud::Aws { lambda, .. }, Action::Apply) => {
                alien_gate::apply(lambda, function).await?;
            }
            (Cloud::Aws { lambda, .. }, Action::Check) => {
                let back = FunctionStore::get(lambda, &function.name).await?;
                alien_gate::check_read_back(function, &back)?;
            }
            (Cloud::Gcp { gcp, .. }, Action::Apply) => {
                if function.architecture != "x86_64" {
                    return Err(AlienError::new(ErrorData::Refused {
                        reason: "Cloud Run runs only x86_64".to_string(),
                    }));
                }
                cloud_run::apply(gcp, function).await?;
            }
            (Cloud::Gcp { gcp, .. }, Action::Check) => {
                let back = cloud_run::ServiceStore::get(gcp, &function.name).await?;
                cloud_run::check_read_back(function, &back)?;
            }
            (Cloud::Azure { azure, .. }, Action::Apply) => {
                if function.architecture != "x86_64" {
                    return Err(AlienError::new(ErrorData::Refused {
                        reason: "Container Apps runs only x86_64".to_string(),
                    }));
                }
                container_apps::apply(azure, function).await?;
            }
            (Cloud::Azure { azure, .. }, Action::Check) => {
                let back = container_apps::AppStore::get(azure, &function.name).await?;
                container_apps::check_read_back(function, &back)?;
            }
        }
        if request.action == Action::Apply {
            self.store_state(&planned.state).await?;
            info!(function = %function.name, digest = %function.digest, "applied the signed function");
        }
        Ok(json!({
            "function": function.name,
            "imageUri": function.image_uri,
            "digest": function.digest,
        }))
    }
}

async fn ssm_inputs(ssm: &SsmClient, name: &str) -> Result<BTreeMap<String, String>> {
    let response = ssm
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

/// The outer reason and what caused it, as one line.
fn describe(error: &AlienError<ErrorData>) -> String {
    let causes = std::iter::successors(error.source.as_deref(), |cause| cause.source.as_deref())
        .map(|cause| cause.message.clone());
    std::iter::once(error.message.clone())
        .chain(causes)
        .collect::<Vec<_>>()
        .join(": ")
}

async fn serve_http(gate: Arc<Gate>) -> std::result::Result<(), lambda_runtime::Error> {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(move |axum::Json(event): axum::Json<Value>| {
            let gate = gate.clone();
            async move {
                match gate.handle(event).await {
                    Ok(body) => (StatusCode::OK, axum::Json(body)).into_response(),
                    Err(error) => {
                        let status = StatusCode::from_u16(error.http_status_code.unwrap_or(500))
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        (
                            status,
                            axum::Json(json!({"errorMessage": describe(&error)})),
                        )
                            .into_response()
                    }
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> std::result::Result<(), lambda_runtime::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .json()
        .init();
    let gate = Arc::new(
        Gate::from_env()
            .await
            .map_err(|error| lambda_runtime::Error::from(describe(&error)))?,
    );
    if matches!(gate.cloud, Cloud::Gcp { .. } | Cloud::Azure { .. }) {
        return serve_http(gate).await;
    }
    let gate = &gate;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        gate.handle(event.payload)
            .await
            .map_err(|error| lambda_runtime::Error::from(describe(&error)))
    }))
    .await
}
