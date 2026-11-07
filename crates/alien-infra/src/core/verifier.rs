//! Function writes on protected installs.
//!
//! A protected install's management identity may not decide what a function runs: setup denies
//! it every Lambda write (AWS), Cloud Run service write (GCP) and Container App write (Azure).
//! The install's `<prefix>-verifier` makes them, from the signed release. A controller hands it
//! the release, the project's signer list chain and the deployment's signed config exactly as
//! the vendor signed them, plus the values the release lets the deployment provide; the
//! verifier checks the statements against the install's own settings and ledger, so nothing
//! here is trusted by it.
//!
//! Request bodies follow the verifier's own types (alien-verifier `Request`, which refuses
//! unknown fields). Unprotected installs never reach this module.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use alien_aws_clients::lambda::{InvocationType, InvokeRequest, LambdaApi};
use alien_core::{GcpClientConfig, GcpCredentials, ReleaseStatements, SignedStatement};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::iam::{GenerateIdTokenRequest, IamApi};
use alien_signing::ReleaseBody;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};

/// The signed release and signer lists of a protected install, or `None` when the install is
/// not protected. A protected install the runner gave no signed release is an error: management
/// may not write its functions itself.
pub fn protected_statements<'a>(
    ctx: &'a ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<Option<&'a ReleaseStatements>> {
    if ctx.deployment_config.stack_settings.signing.is_none() {
        return Ok(None);
    }
    ctx.deployment_config
        .release_statements
        .as_ref()
        .map(Some)
        .ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "This install is protected, but the release being deployed carries no signed statement; only its verifier may change what the function runs".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })
}

/// `applyRelease` for one function workload of the signed release.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRelease {
    pub release: SignedStatement,
    pub signer_lists: Vec<SignedStatement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    /// Values of the names the signed workload lists as provided, by workload.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub provided_env: BTreeMap<String, BTreeMap<String, String>>,
    /// Where this install's copy of the workload's image lives, when it isn't the signed
    /// reference itself. The verifier refuses one whose digest isn't the signed one.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub images: BTreeMap<String, String>,
    /// The one workload this request writes; the release's other functions keep what runs.
    pub workloads: BTreeSet<String>,
}

impl ApplyRelease {
    /// The request that writes `workload` from `statements` and `config`. Of `env`, which the
    /// controller rendered as it would for an unprotected install, only the names the signed
    /// workload lists as provided are sent; the verifier computes everything else itself.
    /// `image` is where the function pulls the image from in this install.
    pub fn for_workload(
        statements: &ReleaseStatements,
        config: Option<&SignedStatement>,
        workload: &str,
        env: &HashMap<String, String>,
        image: &str,
    ) -> Result<Self> {
        let refuse = |message: String| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message,
                resource_id: Some(workload.to_string()),
            })
        };
        let body: ReleaseBody = statements
            .release
            .open()
            .and_then(|opened| opened.body())
            .map_err(|error| refuse(format!("The signed release can't be read: {error}")))?;
        let signed = body
            .workloads
            .iter()
            .find(|candidate| candidate.name == workload)
            .ok_or_else(|| {
                refuse(format!(
                    "The signed release has no workload '{workload}', so the verifier can't write it"
                ))
            })?;
        let provided: BTreeMap<String, String> = env
            .iter()
            .filter(|(name, _)| signed.provided_env.contains(name))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        Ok(Self {
            release: statements.release.clone(),
            signer_lists: statements.signer_lists.clone(),
            config: config.cloned(),
            provided_env: if provided.is_empty() {
                BTreeMap::new()
            } else {
                BTreeMap::from([(workload.to_string(), provided)])
            },
            images: if image == signed.image {
                BTreeMap::new()
            } else {
                BTreeMap::from([(workload.to_string(), image.to_string())])
            },
            workloads: BTreeSet::from([workload.to_string()]),
        })
    }
}

/// One verifier request, as the verifier deserializes it: `{"<kind>": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Request {
    ApplyRelease(ApplyRelease),
}

/// One function the verifier wrote.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedFunction {
    /// The function's name in the cloud.
    pub function: String,
    /// The image it now runs.
    pub image_uri: String,
    /// The signed digest.
    pub digest: String,
}

/// What `applyRelease` did.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Applied {
    /// The sequence of the release the verifier accepted.
    pub release: u64,
    /// The functions it wrote and read back.
    pub functions: Vec<AppliedFunction>,
}

/// The code the verifier gives a refusal: the request or its statements don't allow the write.
/// Any other code is a failure applying again may get past.
const REFUSED: &str = "VERIFIER_REFUSED";

/// Why a call to the verifier didn't produce an answer.
#[derive(Debug)]
pub enum CallError {
    /// The verifier answered with an error: its code and what it reported.
    Answered { code: String, reason: String },
    /// The call itself failed: the verifier was unreachable or answered with something that
    /// isn't an answer.
    Failed(AlienError<ErrorData>),
}

/// The install's verifier.
#[async_trait]
pub trait Verifier: Send + Sync {
    /// Its name, `<prefix>-verifier`.
    fn name(&self) -> &str;
    /// Send one request; the verifier's answer, or why there is none.
    async fn call(&self, request: &Request) -> std::result::Result<serde_json::Value, CallError>;
}

/// Write the function of `request`'s one workload through `verifier`, and check that it wrote
/// exactly `function`. A refusal is an error carrying the verifier's reason.
pub async fn apply_release(
    verifier: &dyn Verifier,
    request: ApplyRelease,
    resource_id: &str,
    function: &str,
) -> Result<Applied> {
    let answer = match verifier.call(&Request::ApplyRelease(request)).await {
        Ok(answer) => answer,
        Err(CallError::Answered { code, reason }) if code == REFUSED => {
            return Err(AlienError::new(ErrorData::ReleaseRefusedByVerifier {
                resource_id: resource_id.to_string(),
                verifier: verifier.name().to_string(),
                reason,
            }))
        }
        Err(CallError::Answered { code, reason }) => {
            return Err(AlienError::new(ErrorData::VerifierFailed {
                resource_id: resource_id.to_string(),
                verifier: verifier.name().to_string(),
                code,
                reason,
            }))
        }
        Err(CallError::Failed(error)) => return Err(error),
    };
    let applied: Applied = serde_json::from_value(answer).into_alien_error().context(
        ErrorData::CloudPlatformError {
            message: format!(
                "The answer of verifier '{}' to applyRelease is not a list of written functions",
                verifier.name()
            ),
            resource_id: Some(resource_id.to_string()),
        },
    )?;
    let written: Vec<&str> = applied
        .functions
        .iter()
        .map(|applied| applied.function.as_str())
        .collect();
    if written != [function] {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Verifier '{}' wrote {written:?}, not the function '{function}'",
                verifier.name()
            ),
            resource_id: Some(resource_id.to_string()),
        }));
    }
    Ok(applied)
}

/// The AWS verifier: Lambda `<prefix>-verifier`, invoked synchronously with the request as its
/// payload. A function error is the verifier's refusal.
pub struct LambdaVerifier {
    client: Arc<dyn LambdaApi>,
    function_name: String,
}

impl LambdaVerifier {
    pub fn new(client: Arc<dyn LambdaApi>, resource_prefix: &str) -> Self {
        Self {
            client,
            function_name: format!("{resource_prefix}-verifier"),
        }
    }
}

/// An error as the verifier reports it: through the Lambda runtime, or as the HTTP body.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportedError {
    #[serde(default)]
    error_type: String,
    error_message: String,
}

impl ReportedError {
    /// What the verifier reported in `body`. A body that names no code is reported under
    /// `fallback_code`, and one that isn't a report at all is the reason as is.
    fn read(body: &[u8], fallback_code: String) -> CallError {
        match serde_json::from_slice::<ReportedError>(body) {
            Ok(reported) if !reported.error_type.is_empty() => CallError::Answered {
                code: reported.error_type,
                reason: reported.error_message,
            },
            Ok(reported) => CallError::Answered {
                code: fallback_code,
                reason: reported.error_message,
            },
            Err(_) => CallError::Answered {
                code: fallback_code,
                reason: String::from_utf8_lossy(body).into_owned(),
            },
        }
    }
}

#[async_trait]
impl Verifier for LambdaVerifier {
    fn name(&self) -> &str {
        &self.function_name
    }

    async fn call(&self, request: &Request) -> std::result::Result<serde_json::Value, CallError> {
        let failed = |message: String| ErrorData::CloudPlatformError {
            message,
            resource_id: None,
        };
        let payload = serde_json::to_vec(request)
            .into_alien_error()
            .context(failed(
                "The verifier request could not be encoded".to_string(),
            ))
            .map_err(CallError::Failed)?;
        let response = self
            .client
            .invoke(
                InvokeRequest::builder()
                    .function_name(self.function_name.clone())
                    .invocation_type(InvocationType::RequestResponse)
                    .payload(payload)
                    .build(),
            )
            .await
            .context(failed(format!(
                "Failed to call the verifier '{}'",
                self.function_name
            )))
            .map_err(CallError::Failed)?;
        if let Some(kind) = response.function_error {
            return Err(ReportedError::read(&response.payload, kind));
        }
        serde_json::from_slice(&response.payload)
            .into_alien_error()
            .context(failed(format!(
                "The verifier '{}' answered with no JSON",
                self.function_name
            )))
            .map_err(CallError::Failed)
    }
}

/// How management proves who it is to an HTTP verifier.
#[async_trait]
pub trait VerifierToken: Send + Sync {
    async fn bearer(&self) -> Result<String>;
}

/// A Google ID token for management's service account, with the verifier's service URL as its
/// audience. Cloud Run lets it through when setup granted that account `roles/run.invoker` on
/// the service.
pub struct GcpIdToken {
    pub iam: Arc<dyn IamApi>,
    pub service_account: String,
    pub audience: String,
}

#[async_trait]
impl VerifierToken for GcpIdToken {
    async fn bearer(&self) -> Result<String> {
        let response = self
            .iam
            .generate_id_token(
                self.service_account.clone(),
                GenerateIdTokenRequest::builder()
                    .audience(self.audience.clone())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to get an ID token for the verifier as '{}'",
                    self.service_account
                ),
                resource_id: None,
            })?;
        Ok(response.token)
    }
}

/// The URL Cloud Run gives the GCP verifier service `<prefix>-verifier`.
pub fn cloud_run_verifier_url(resource_prefix: &str, project_number: &str, region: &str) -> String {
    format!("https://{resource_prefix}-verifier-{project_number}.{region}.run.app")
}

/// The service account management runs as on GCP, whose ID token Cloud Run lets through to the
/// verifier.
fn gcp_management_service_account(config: &GcpClientConfig) -> Result<String> {
    match &config.credentials {
        GcpCredentials::Federated { access, .. } => Ok(access.service_account_email.clone()),
        GcpCredentials::ImpersonatedServiceAccount { config, .. } => {
            Ok(config.service_account_email.clone())
        }
        _ => Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "A protected GCP install calls its verifier as the management service account, and these credentials name none".to_string(),
            resource_id: None,
        })),
    }
}

/// The install's GCP verifier: Cloud Run service `<prefix>-verifier`, called with an ID token for
/// management's service account (setup grants it `roles/run.invoker` on that service).
pub fn gcp_verifier(ctx: &ResourceControllerContext<'_>) -> Result<HttpVerifier> {
    let config = ctx.get_gcp_config()?;
    let name = format!("{}-verifier", ctx.resource_prefix);
    let url = match config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("verifier"))
    {
        Some(url) => url.clone(),
        None => {
            let project_number = config.project_number.as_deref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "The GCP project number is needed to reach the verifier, and the credentials carry none".to_string(),
                    resource_id: None,
                })
            })?;
            cloud_run_verifier_url(ctx.resource_prefix, project_number, &config.region)
        }
    };
    let token = GcpIdToken {
        iam: ctx.service_provider.get_gcp_iam_client(config)?,
        service_account: gcp_management_service_account(config)?,
        audience: url.clone(),
    };
    Ok(HttpVerifier::new(
        reqwest::Client::new(),
        name,
        url,
        Some(Arc::new(token)),
    ))
}

/// The install's Azure verifier: Container App `<prefix>-verifier` in the install's resource
/// group, at the ingress address Azure gave it.
pub async fn azure_verifier(ctx: &ResourceControllerContext<'_>) -> Result<HttpVerifier> {
    let config = ctx.get_azure_config()?;
    let name = format!("{}-verifier", ctx.resource_prefix);
    if let Some(url) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("verifier"))
    {
        return Ok(HttpVerifier::new(
            reqwest::Client::new(),
            name,
            url.clone(),
            None,
        ));
    }
    let resource_group = crate::azure_utils::get_resource_group_name(ctx.state)?;
    let app = ctx
        .service_provider
        .get_azure_container_apps_client(config)?
        .get_container_app(&resource_group, &name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to read the verifier Container App '{name}'"),
            resource_id: None,
        })?;
    let fqdn = app
        .properties
        .as_ref()
        .and_then(|properties| properties.configuration.as_ref())
        .and_then(|configuration| configuration.ingress.as_ref())
        .and_then(|ingress| ingress.fqdn.clone())
        .filter(|fqdn| !fqdn.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("The verifier Container App '{name}' has no ingress address"),
                resource_id: None,
            })
        })?;
    Ok(HttpVerifier::new(
        reqwest::Client::new(),
        name,
        format!("https://{fqdn}"),
        None,
    ))
}

/// The GCP and Azure verifier: `POST /` on the Cloud Run service or Container App with the same
/// JSON the AWS Lambda takes. 200 carries the answer; any other status is a refusal, with
/// `{"errorMessage": ...}`.
pub struct HttpVerifier {
    http: reqwest::Client,
    name: String,
    url: String,
    /// GCP: management's ID token for the Cloud Run service. Azure: none; the Container App's
    /// ingress takes any caller, who can only hand it statements the vendor signed.
    token: Option<Arc<dyn VerifierToken>>,
}

impl HttpVerifier {
    pub fn new(
        http: reqwest::Client,
        name: impl Into<String>,
        url: impl Into<String>,
        token: Option<Arc<dyn VerifierToken>>,
    ) -> Self {
        Self {
            http,
            name: name.into(),
            url: url.into(),
            token,
        }
    }
}

#[async_trait]
impl Verifier for HttpVerifier {
    fn name(&self) -> &str {
        &self.name
    }

    async fn call(&self, request: &Request) -> std::result::Result<serde_json::Value, CallError> {
        let failed = |message: String| ErrorData::CloudPlatformError {
            message,
            resource_id: None,
        };
        let mut post = self
            .http
            .post(format!("{}/", self.url.trim_end_matches('/')))
            .json(request);
        if let Some(token) = &self.token {
            post = post.bearer_auth(token.bearer().await.map_err(CallError::Failed)?);
        }
        let response = post
            .send()
            .await
            .into_alien_error()
            .context(failed(format!(
                "Failed to call the verifier at '{}'",
                self.url
            )))
            .map_err(CallError::Failed)?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .into_alien_error()
            .context(failed(format!(
                "Failed to read the answer of the verifier at '{}'",
                self.url
            )))
            .map_err(CallError::Failed)?;
        if !status.is_success() {
            return Err(ReportedError::read(&body, format!("HTTP {status}")));
        }
        serde_json::from_slice(&body)
            .into_alien_error()
            .context(failed(format!(
                "The verifier at '{}' answered with no JSON",
                self.url
            )))
            .map_err(CallError::Failed)
    }
}
