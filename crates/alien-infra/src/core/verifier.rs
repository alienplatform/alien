//! Function writes on protected installs.
//!
//! A protected install's management identity may not decide what a function runs: setup denies
//! it every Lambda write (AWS), Cloud Run service write (GCP) and Container App write (Azure).
//! The install's `<prefix>-verifier` makes them. A controller builds the same cloud request it
//! sends on an unprotected install and hands it to the verifier with the release, the project's
//! signer list chain and the deployment's signed config exactly as the vendor signed them; the
//! verifier checks the statements against the install's own settings and ledger and every field
//! of the request against the signed release, so nothing here is trusted by it.
//!
//! Request bodies follow the verifier's own types (alien-verifier `Request`, which refuses
//! unknown fields). Unprotected installs never reach this module.

use std::collections::HashMap;
use std::sync::Arc;

use alien_aws_clients::lambda::{InvocationType, InvokeRequest, LambdaApi};
use alien_azure_clients::AzureClientConfigExt as _;
use alien_core::{
    AzureClientConfig, GcpClientConfig, GcpCredentials, ReleaseStatements, SignedStatement,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_gcp_clients::iam::{GenerateIdTokenRequest, IamApi};
use alien_signing::{binding_env_name, ReleaseBody};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::info;

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
                message: "This install is protected, and this update carries no signed release. Protected installs update only through their manager, which hands the signed release to the install's verifier; a runner that takes the deployment itself (such as `alien deploy up`) can't update them".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })
}

/// Whether the install is protected: its Live functions are created, changed and deleted only
/// by its verifier.
pub fn is_protected(ctx: &ResourceControllerContext<'_>) -> bool {
    ctx.deployment_config.stack_settings.signing.is_some()
}

/// Env var with the secrets a protected function loads at start: the signed workload's secret
/// names and the release digest. Unprotected functions keep `ALIEN_SECRETS`, which the
/// deployment writes from its own variables; the verifier refuses that one on a protected
/// install, since it would let management pick which vault keys load.
pub const WORKLOAD_SECRETS: &str = "WORKLOAD_SECRETS";

/// Render into `env`, the environment a controller built for `workload`, what the signed
/// statements decide on a protected install: the signed values, the secrets list it loads at
/// start (`WORKLOAD_SECRETS` from the signed workload, in place of the deployment's
/// `ALIEN_SECRETS`) and the values the signed `config` sets for the names the release makes
/// configurable. The deployment's telemetry goes to the signed config's endpoint, or nowhere.
/// Anything else the deployment holds (such as variables set in the dashboard before the
/// install was protected) is left out: only names the release lets the deployment provide,
/// and the bindings the verifier derives, stay. `deployer_values` are the deployment's input
/// values: the signed config's developer inputs that are conditional on a deployer input apply
/// as that input is set, and the verifier checks the result against its own read of them.
pub fn apply_signed_values(
    statements: &ReleaseStatements,
    config: Option<&SignedStatement>,
    workload: &str,
    deployer_values: &HashMap<String, serde_json::Value>,
    env: &mut HashMap<String, String>,
) -> Result<()> {
    let refuse = |message: String| {
        AlienError::new(ErrorData::ResourceConfigInvalid {
            message,
            resource_id: Some(workload.to_string()),
        })
    };
    let opened = statements
        .release
        .open()
        .map_err(|error| refuse(format!("The signed release can't be read: {error}")))?;
    let body: ReleaseBody = opened
        .body()
        .map_err(|error| refuse(format!("The signed release can't be read: {error}")))?;
    let signed = body
        .workloads
        .iter()
        .find(|candidate| candidate.name == workload)
        .ok_or_else(|| refuse(format!("The signed release has no workload '{workload}'")))?;

    env.remove(alien_core::ENV_ALIEN_SECRETS);
    if !signed.secrets.is_empty() {
        let keys: Vec<&str> = signed
            .secrets
            .iter()
            .map(|secret| secret.name.as_str())
            .collect();
        env.insert(
            WORKLOAD_SECRETS.to_string(),
            serde_json::json!({ "keys": keys, "hash": opened.digest }).to_string(),
        );
    }

    let config = config
        .map(|config| {
            config
                .open()
                .and_then(|opened| opened.body::<alien_signing::ConfigBody>())
                .map_err(|error| refuse(format!("The signed config can't be read: {error}")))
        })
        .transpose()?
        .unwrap_or_default();
    let configured =
        alien_core::release_statement::signed_config_env(&body, workload, &config, deployer_values)
            .map_err(|error| refuse(format!("The signed config can't be applied: {error}")))?;
    for (name, value) in &configured {
        if signed.configurable_env.contains(name) {
            env.insert(name.clone(), value.clone());
        }
    }

    // Telemetry: the deployment decides whether its workloads send any, the signed config
    // where. Without a signed endpoint they send none. Metrics have no signed endpoint, so
    // the deployment's never goes out; the release may still sign one into the env.
    match (
        &config.telemetry_endpoint,
        env.contains_key(OTLP_LOGS_ENDPOINT),
    ) {
        (Some(endpoint), true) => {
            env.insert(OTLP_LOGS_ENDPOINT.to_string(), endpoint.clone());
        }
        _ => {
            env.remove(OTLP_LOGS_ENDPOINT);
            for name in TELEMETRY_FOLLOWERS {
                env.remove(*name);
            }
        }
    }
    match signed.env.get(OTLP_METRICS_ENDPOINT) {
        Some(value) => env.insert(OTLP_METRICS_ENDPOINT.to_string(), value.clone()),
        None => env.remove(OTLP_METRICS_ENDPOINT),
    };

    for (name, value) in &signed.env {
        env.insert(name.clone(), value.clone());
    }
    let bindings: Vec<String> = signed
        .bindings
        .keys()
        .map(|id| binding_env_name(id))
        .chain([binding_env_name(workload), binding_env_name(SECRETS_VAULT)])
        .collect();
    let mut left_out: Vec<String> = env
        .keys()
        .filter(|name| {
            let name = name.as_str();
            !(signed.env.contains_key(name)
                || (signed
                    .configurable_env
                    .iter()
                    .any(|configurable| configurable == name)
                    && configured.contains_key(name))
                || signed.provided_env.iter().any(|provided| provided == name)
                || signed.deployer_env.contains_key(name)
                || bindings.iter().any(|binding| binding == name)
                || RENDERED_HERE.contains(&name))
        })
        .cloned()
        .collect();
    left_out.sort();
    for name in &left_out {
        env.remove(name);
    }
    if !left_out.is_empty() {
        info!(
            workload,
            ?left_out,
            "Left out env the signed release doesn't let the deployment set"
        );
    }
    Ok(())
}

/// The install's `secrets` vault, which every protected function's runtime reads.
const SECRETS_VAULT: &str = "secrets";

/// What [`apply_signed_values`] renders itself, which the verifier checks by its own rules: the
/// secrets list and telemetry.
const RENDERED_HERE: &[&str] = &[
    WORKLOAD_SECRETS,
    OTLP_LOGS_ENDPOINT,
    OTLP_METRICS_ENDPOINT,
    "OTEL_SERVICE_NAME",
    "OTEL_RESOURCE_ATTRIBUTES",
    alien_core::ENV_ALIEN_RUNTIME_SECRETS,
    alien_core::ENV_ALIEN_RUNTIME_SEND_OTLP,
];

/// Where a workload sends its logs; on a protected install only the signed config's
/// `telemetryEndpoint` may be it.
const OTLP_LOGS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT";
/// Where a workload sends its metrics, which the signed config has no endpoint for.
const OTLP_METRICS_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT";
/// What the deployment sets with its logs endpoint, and the verifier lets through only with
/// one: the service's name and attributes, and the runtime's pointer to its telemetry
/// credentials.
const TELEMETRY_FOLLOWERS: &[&str] = &[
    "OTEL_SERVICE_NAME",
    "OTEL_RESOURCE_ATTRIBUTES",
    alien_core::ENV_ALIEN_RUNTIME_SECRETS,
];

/// The digest of the signed statements the step carries: the release and the config, if any.
/// A worker's env follows both (the secrets list names the release's digest), so a worker
/// updated for another release gets its configuration written with its code. `None` on an
/// install that isn't protected.
pub fn statements_digest(ctx: &ResourceControllerContext<'_>) -> Result<Option<String>> {
    let Some(statements) = &ctx.deployment_config.release_statements else {
        return Ok(None);
    };
    let digest = |statement: &alien_core::SignedStatement, what: &str| {
        statement
            .open()
            .map(|opened| opened.digest)
            .context(ErrorData::ResourceConfigInvalid {
                message: format!("The install's signed {what} can't be read"),
                resource_id: None,
            })
    };
    let release = digest(&statements.release, "release")?;
    let config = ctx
        .deployment_config
        .signed_config
        .as_ref()
        .map(|statement| digest(statement, "config"))
        .transpose()?;
    Ok(Some(format!("{release}+{}", config.unwrap_or_default())))
}

/// Whether a protected worker that last had the signed statements `applied` applied must go to
/// the verifier again: the step carries another release or config. Neither is part of the
/// worker's own configuration, so nothing else would update it.
pub fn statements_changed(
    ctx: &ResourceControllerContext<'_>,
    applied: Option<&str>,
) -> Result<bool> {
    if ctx.deployment_config.stack_settings.signing.is_none() {
        return Ok(false);
    }
    Ok(statements_digest(ctx)?.is_some_and(|digest| Some(digest.as_str()) != applied))
}

/// One cloud write a controller hands the verifier, exactly as it would send it to the cloud
/// on an unprotected install (alien-verifier `CloudWrite`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloudWrite {
    /// Lambda CreateFunction.
    CreateFunction(serde_json::Value),
    /// Lambda UpdateFunctionCode.
    #[serde(rename_all = "camelCase")]
    UpdateFunctionCode {
        function_name: String,
        request: serde_json::Value,
    },
    /// Lambda UpdateFunctionConfiguration.
    #[serde(rename_all = "camelCase")]
    UpdateFunctionConfiguration {
        function_name: String,
        request: serde_json::Value,
    },
    /// Lambda AddPermission.
    #[serde(rename_all = "camelCase")]
    AddPermission {
        function_name: String,
        request: serde_json::Value,
    },
    /// Lambda PutFunctionConcurrency.
    #[serde(rename_all = "camelCase")]
    PutFunctionConcurrency {
        function_name: String,
        reserved_concurrent_executions: u32,
    },
    /// Lambda DeleteFunctionConcurrency.
    #[serde(rename_all = "camelCase")]
    DeleteFunctionConcurrency { function_name: String },
    /// Cloud Run services.create.
    #[serde(rename_all = "camelCase")]
    CreateService {
        service_id: String,
        service: serde_json::Value,
    },
    /// Cloud Run services.patch of the whole service.
    #[serde(rename_all = "camelCase")]
    ReplaceService {
        name: String,
        service: serde_json::Value,
    },
    /// Cloud Run services.setIamPolicy.
    #[serde(rename_all = "camelCase")]
    SetServicePolicy {
        name: String,
        policy: serde_json::Value,
    },
    /// Container Apps createOrUpdate.
    #[serde(rename_all = "camelCase")]
    CreateContainerApp {
        name: String,
        app: serde_json::Value,
    },
    /// Container Apps update.
    #[serde(rename_all = "camelCase")]
    UpdateContainerApp {
        name: String,
        app: serde_json::Value,
    },
}

impl CloudWrite {
    /// `request`, serialized as the cloud client sends it.
    pub fn body<T: Serialize>(request: &T) -> Result<serde_json::Value> {
        serde_json::to_value(request)
            .into_alien_error()
            .context(ErrorData::CloudPlatformError {
                message: "A cloud request can't be serialized for the verifier".to_string(),
                resource_id: None,
            })
    }
}

/// `applyWrite`: the cloud writes for one signed workload, in order, with the statements they
/// rest on. The verifier checks every write before it sends any.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyWrite {
    pub release: SignedStatement,
    pub signer_lists: Vec<SignedStatement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    pub workload: String,
    pub writes: Vec<CloudWrite>,
}

/// What `applyWrite` did.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Written {
    /// The sequence of the release the verifier accepted.
    pub release: u64,
    /// The function it wrote.
    pub function: String,
    /// What the cloud answered to each write, in order, where the controller needs it (a
    /// created function's ARN).
    pub answers: Vec<serde_json::Value>,
}

/// One verifier request, as the verifier deserializes it: `{"<kind>": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Request {
    ApplyWrite(ApplyWrite),
    DeleteWorkload(DeleteWorkload),
}

/// Delete a Live workload's function, service or app. Deleting needs no signed statement.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWorkload {
    pub workload: String,
}

/// What `deleteWorkload` did.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Deleted {
    /// The function, service or app it deleted.
    pub deleted: String,
    /// Whether it still existed.
    pub existed: bool,
}

/// The code the verifier gives a refusal: the request or its statements don't allow the write.
/// Any other code is a failure applying again may get past.
const REFUSED: &str = "VERIFIER_REFUSED";
/// The code the verifier gives a cloud quota with no room for what the release asks: applying
/// again gets the same answer until the quota is raised.
const QUOTA_EXCEEDED: &str = "VERIFIER_QUOTA_EXCEEDED";

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

/// The deployment's error for the verifier's answer `code`/`reason`: a refusal never changes
/// on retry, nor does a quota with no room; anything else may get past on the next apply.
fn answered(
    verifier: &dyn Verifier,
    resource_id: &str,
    code: String,
    reason: String,
) -> AlienError<ErrorData> {
    if code == REFUSED {
        return AlienError::new(ErrorData::ReleaseRefusedByVerifier {
            resource_id: resource_id.to_string(),
            verifier: verifier.name().to_string(),
            reason,
        });
    }
    let retryable = code != QUOTA_EXCEEDED;
    let mut error = AlienError::new(ErrorData::VerifierFailed {
        resource_id: resource_id.to_string(),
        verifier: verifier.name().to_string(),
        code,
        reason,
    });
    error.retryable = retryable;
    error
}

/// Hand `writes`, the cloud requests a controller built for `workload`'s function for one
/// release, in order, to `verifier`, which checks every one against the signed `statements`
/// and `config` before it sends any, sends them unchanged and reads them back. A refusal is an
/// error carrying the verifier's reason, and nothing was written.
pub async fn apply_write(
    verifier: &dyn Verifier,
    statements: &ReleaseStatements,
    config: Option<&SignedStatement>,
    workload: &str,
    writes: Vec<CloudWrite>,
    function: &str,
) -> Result<Written> {
    let count = writes.len();
    let request = Request::ApplyWrite(ApplyWrite {
        release: statements.release.clone(),
        signer_lists: statements.signer_lists.clone(),
        config: config.cloned(),
        workload: workload.to_string(),
        writes,
    });
    let answer = match verifier.call(&request).await {
        Ok(answer) => answer,
        Err(CallError::Answered { code, reason }) => {
            return Err(answered(verifier, workload, code, reason))
        }
        Err(CallError::Failed(error)) => return Err(error),
    };
    let written: Written = serde_json::from_value(answer).into_alien_error().context(
        ErrorData::CloudPlatformError {
            message: format!(
                "The answer of verifier '{}' to applyWrite is not the writes to one function",
                verifier.name()
            ),
            resource_id: Some(workload.to_string()),
        },
    )?;
    if written.answers.len() != count {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Verifier '{}' answered {} of the {count} writes it was sent",
                verifier.name(),
                written.answers.len()
            ),
            resource_id: Some(workload.to_string()),
        }));
    }
    if written.function != function {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Verifier '{}' wrote '{}', not the function '{function}'",
                verifier.name(),
                written.function
            ),
            resource_id: Some(workload.to_string()),
        }));
    }
    Ok(written)
}

/// Have `verifier` delete `workload`'s function, service or app, `function`, with what the cloud
/// attaches to it. A refusal is an error carrying the verifier's reason.
pub async fn delete_workload(
    verifier: &dyn Verifier,
    workload: &str,
    function: &str,
) -> Result<Deleted> {
    let request = Request::DeleteWorkload(DeleteWorkload {
        workload: workload.to_string(),
    });
    let answer = match verifier.call(&request).await {
        Ok(answer) => answer,
        Err(CallError::Answered { code, reason }) => {
            return Err(answered(verifier, workload, code, reason))
        }
        Err(CallError::Failed(error)) => return Err(error),
    };
    let deleted: Deleted = serde_json::from_value(answer).into_alien_error().context(
        ErrorData::CloudPlatformError {
            message: format!(
                "The answer of verifier '{}' to deleteWorkload is not a deletion",
                verifier.name()
            ),
            resource_id: Some(workload.to_string()),
        },
    )?;
    if deleted.deleted != function {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Verifier '{}' deleted '{}', not '{function}'",
                verifier.name(),
                deleted.deleted
            ),
            resource_id: Some(workload.to_string()),
        }));
    }
    Ok(deleted)
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

/// The management identity's token for Azure Resource Manager. Container Apps authentication on
/// the Azure verifier lets a request through only with this identity's token for that audience;
/// every credential management has can mint it, so setup needs no app registration.
pub struct AzureManagementToken {
    pub config: AzureClientConfig,
}

#[async_trait]
impl VerifierToken for AzureManagementToken {
    async fn bearer(&self) -> Result<String> {
        self.config
            .get_bearer_token()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get the management identity's token for the verifier"
                    .to_string(),
                resource_id: None,
            })
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
/// group, at the ingress address Azure gave it, called with the management identity's token.
pub async fn azure_verifier(ctx: &ResourceControllerContext<'_>) -> Result<HttpVerifier> {
    let config = ctx.get_azure_config()?;
    let name = format!("{}-verifier", ctx.resource_prefix);
    let token: Arc<dyn VerifierToken> = Arc::new(AzureManagementToken {
        config: config.clone(),
    });
    if let Some(url) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("verifier"))
    {
        return Ok(HttpVerifier::new(
            reqwest::Client::new(),
            name,
            url.clone(),
            Some(token),
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
        Some(token),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::verifier_test_support::{signed_config, signed_release_configuring};
    use alien_core::{Platform, ResourceLifecycle, Stack, Worker, WorkerCode};

    /// The deployment's own secrets list never reaches a protected function, and the signed
    /// config sets only the names the release makes configurable.
    #[test]
    fn a_protected_env_takes_the_signed_values() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: format!("ghcr.io/shop/api@sha256:{}", "1".repeat(64)),
            })
            .permissions("execution".to_string())
            .build();
        let statements = signed_release_configuring(
            Platform::Aws,
            &Stack::new("shop".to_string())
                .add(worker, ResourceLifecycle::Live)
                .build(),
            1,
            &["LOG_LEVEL"],
        );
        let install = alien_signing::InstallId::Aws {
            account: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            prefix: "shop".to_string(),
        };
        let config = signed_config(&install, &[("LOG_LEVEL", "debug")], 1);
        let mut env = HashMap::from([
            (
                alien_core::ENV_ALIEN_SECRETS.to_string(),
                r#"{"keys":["LD_PRELOAD"],"hash":"x"}"#.to_string(),
            ),
            ("LOG_LEVEL".to_string(), "info".to_string()),
            ("ALIEN_DEPLOYMENT_ID".to_string(), "dep_1".to_string()),
        ]);

        apply_signed_values(&statements, Some(&config), "api", &HashMap::new(), &mut env).unwrap();

        assert_eq!(
            env,
            HashMap::from([
                ("LOG_LEVEL".to_string(), "debug".to_string()),
                ("ALIEN_DEPLOYMENT_ID".to_string(), "dep_1".to_string()),
            ])
        );
        let error =
            apply_signed_values(&statements, None, "jobs", &HashMap::new(), &mut env).unwrap_err();
        assert!(
            error.to_string().contains("has no workload 'jobs'"),
            "{error}"
        );
    }
}
