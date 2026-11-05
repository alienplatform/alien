//! The verifier as a Lambda function on AWS, a Cloud Run service on GCP or a
//! Container App on Azure.
//!
//! It reads its trust from the install, never from here. Setup creates it
//! with this configuration, which only setup can change:
//!
//! - `VERIFIER_CLOUD`: `aws`, `gcp` or `azure`;
//! - on AWS, `VERIFIER_FUNCTION_VPC`: `{"subnetIds": [..], "securityGroupIds": [..]}`
//!   when setup puts functions in a VPC, and `VERIFIER_MACHINE_NETWORK`, the
//!   same for the subnets and security groups setup created for machines
//!   when the install runs any. The verifier's own name, account and region
//!   come from the ARN Lambda invokes it with;
//! - on GCP the project and region come from the metadata server, and the
//!   name from Cloud Run's `K_SERVICE`;
//! - on Azure, `VERIFIER_AZURE_RESOURCE_GROUP`
//!   (`/subscriptions/<id>/resourceGroups/<name>`), `VERIFIER_AZURE_VAULT`
//!   (`https://<vault>.vault.azure.net`, the install's inputs vault) and, when
//!   the verifier has several identities, `VERIFIER_AZURE_CLIENT_ID`; the name
//!   comes from Container Apps' `CONTAINER_APP_NAME`.
//!
//! On GCP and Azure the verifier serves `POST /` on `$PORT`; the platform's
//! own authentication decides who may call it.

#![expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]

use std::sync::Arc;

use alien_aws_clients::autoscaling::AutoScalingClient;
use alien_aws_clients::ec2::Ec2Client;
use alien_aws_clients::lambda::LambdaClient;
use alien_aws_clients::ssm::SsmClient;
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::InstallId;
use alien_verifier::aws::{self, AwsBackend, SsmInstall};
use alien_verifier::azure::{Azure, AzureBackend, AzureInstall};
use alien_verifier::gcp::{self, Gcp, GcpBackend, GcpInstall, TokenSource};
use alien_verifier::machines::AwsMachines;
use alien_verifier::plan::SetupNetwork;
use alien_verifier::trust::InstallStore;
use alien_verifier::{describe, Backend, ErrorData, Result, Verifier};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::{json, Value};

fn env(name: &str) -> Result<String> {
    optional_env(name)?.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("{name} is not set"),
        })
    })
}

/// `name`, if it is set. Set to something that isn't UTF-8 is an error.
fn optional_env(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("{name} is not UTF-8"),
            }))
        }
    }
}

/// The subnets and security groups setup put in `name`, if it is set.
fn network_env(name: &str) -> Result<Option<SetupNetwork>> {
    optional_env(name)?
        .map(|text| {
            serde_json::from_str(&text).into_alien_error().context(
                ErrorData::ConfigurationInvalid {
                    message: format!("{name} is not subnet and security group ids"),
                },
            )
        })
        .transpose()
}

struct Aws {
    store: Arc<dyn InstallStore>,
    backend: Arc<dyn Backend>,
    vpc: Option<SetupNetwork>,
    machine_network: Option<SetupNetwork>,
}

impl Aws {
    async fn from_env() -> Result<Self> {
        let config =
            AwsClientConfig::from_std_env()
                .await
                .context(ErrorData::ConfigurationInvalid {
                    message: "AWS credentials are not available".to_string(),
                })?;
        let region = config.region.clone();
        if region.is_empty() {
            return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message: "the AWS region is not set".to_string(),
            }));
        }
        let credentials = AwsCredentialProvider::from_config(config).await.context(
            ErrorData::ConfigurationInvalid {
                message: "AWS credentials are not available".to_string(),
            },
        )?;
        let http = reqwest::Client::new();
        let vpc = network_env("VERIFIER_FUNCTION_VPC")?;
        let machine_network = network_env("VERIFIER_MACHINE_NETWORK")?;
        Ok(Self {
            store: Arc::new(SsmInstall::new(SsmClient::new(
                http.clone(),
                credentials.clone(),
            ))),
            backend: Arc::new(AwsBackend {
                lambda: LambdaClient::new(http.clone(), credentials.clone()),
                machines: AwsMachines {
                    ec2: Ec2Client::new(http.clone(), credentials.clone()),
                    autoscaling: AutoScalingClient::new(http, credentials),
                },
                region,
                images: alien_verifier::machines::ImagePins::aws(),
            }),
            vpc,
            machine_network,
        })
    }

    /// The verifier for one invocation of the function `arn`.
    fn verifier(&self, arn: &str) -> Result<Verifier> {
        let (name, account, region) = aws::identity(arn)?;
        Ok(Verifier {
            own_name: name,
            install_id: Arc::new(move |prefix: &str| aws::install_id(&account, &region, prefix)),
            store: self.store.clone(),
            backend: self.backend.clone(),
            vpc: self.vpc.clone(),
            machine_network: self.machine_network.clone(),
        })
    }
}

async fn gcp_verifier() -> Result<Verifier> {
    let tokens = match optional_env("VERIFIER_GCP_TOKEN_FILE")? {
        Some(path) => TokenSource::File(path),
        None => TokenSource::Metadata,
    };
    let (project, region) = gcp::metadata_identity().await?;
    let client = Gcp::new(tokens, project.clone(), region);
    Ok(Verifier {
        own_name: env("K_SERVICE")?,
        install_id: Arc::new(move |prefix: &str| InstallId::Gcp {
            project: project.clone(),
            prefix: prefix.to_string(),
        }),
        store: Arc::new(GcpInstall(client.clone())),
        backend: Arc::new(GcpBackend(
            client,
            alien_verifier::machines::ImagePins::gcp(),
        )),
        vpc: None,
        machine_network: None,
    })
}

fn azure_verifier() -> Result<Verifier> {
    let resource_group = env("VERIFIER_AZURE_RESOURCE_GROUP")?;
    let (subscription, group) =
        match resource_group
            .trim_start_matches('/')
            .split('/')
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["subscriptions", subscription, "resourceGroups", group] => {
                (subscription.to_string(), group.to_string())
            }
            _ => return Err(AlienError::new(ErrorData::ConfigurationInvalid {
                message:
                    "VERIFIER_AZURE_RESOURCE_GROUP is not /subscriptions/<id>/resourceGroups/<name>"
                        .to_string(),
            })),
        };
    let client = Azure::new(
        resource_group,
        env("VERIFIER_AZURE_VAULT")?,
        optional_env("VERIFIER_AZURE_CLIENT_ID")?,
    );
    Ok(Verifier {
        own_name: env("CONTAINER_APP_NAME")?,
        install_id: Arc::new(move |_prefix: &str| InstallId::Azure {
            subscription: subscription.clone(),
            resource_group: group.clone(),
        }),
        store: Arc::new(AzureInstall(client.clone())),
        backend: Arc::new(AzureBackend(
            client,
            alien_verifier::machines::ImagePins::azure(),
        )),
        vpc: None,
        machine_network: None,
    })
}

async fn serve_http(verifier: Arc<Verifier>) -> std::result::Result<(), lambda_runtime::Error> {
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(move |axum::Json(event): axum::Json<Value>| {
            let verifier = verifier.clone();
            async move {
                match verifier.handle(event).await {
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
    let fail = |error: AlienError<ErrorData>| lambda_runtime::Error::from(describe(&error));
    match env("VERIFIER_CLOUD").map_err(fail)?.as_str() {
        "gcp" => serve_http(Arc::new(gcp_verifier().await.map_err(fail)?)).await,
        "azure" => serve_http(Arc::new(azure_verifier().map_err(fail)?)).await,
        "aws" => {
            let aws = &Aws::from_env().await.map_err(fail)?;
            lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
                let verifier = aws
                    .verifier(&event.context.invoked_function_arn)
                    .map_err(fail)?;
                verifier.handle(event.payload).await.map_err(fail)
            }))
            .await
        }
        other => Err(lambda_runtime::Error::from(format!(
            "VERIFIER_CLOUD '{other}' is not aws, gcp or azure"
        ))),
    }
}
