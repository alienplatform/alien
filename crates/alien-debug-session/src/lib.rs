//! Shared types + cloud-credential translator for `alien debug` sessions.
//!
//! Consumed from three places:
//! - The CLI (`alien-cli/src/commands/debug.rs`) deserializes
//!   [`DebugSessionResponse`] from the manager's `POST /v1/debug/sessions`
//!   reply and uses it to set env + materialize files before `exec`.
//! - The manager's debug route (`platform/crates/alien-managerx/src/routes/debug.rs`)
//!   produces a response by calling [`translate_client_config`] against the
//!   resolver's output (push-mode deployments).
//! - The agent (`alien-agent/src/loops/commands.rs`) produces a response by
//!   calling [`translate_client_config`] against creds it mints **locally**
//!   from its own pod identity (pull-mode deployments — IRSA / Workload
//!   Identity / Managed Identity).
//!
//! Pulling these types + the translator into one OSS crate is what lets the
//! agent fulfill a debug request without the manager having direct cloud
//! access to the customer's environment.
//!
//! ## Coverage today
//!
//! | Platform | Status |
//! | --- | --- |
//! | AWS push | STS `SessionCredentials` → `AWS_ACCESS_KEY_ID` / `_SECRET_ACCESS_KEY` / `_SESSION_TOKEN` / `_REGION`. Expiry surfaced. |
//! | GCP push | Impersonated OAuth2 access token → `CLOUDSDK_AUTH_ACCESS_TOKEN` + `_CORE_PROJECT` / `_COMPUTE_REGION`. No expiry surfaced. |
//! | Azure push | `AZURE_*` env vars + (for WorkloadIdentity) federated-token file + an `az login` setup script. |
//! | Kubernetes / KubernetesCloud | `FeatureNotSupported` here. Agent-side pure-K8s SA-token kubeconfigs are built separately and returned as [`DebugSessionResponse::Pull`]. |
//! | Local / Test | `FeatureNotSupported`. |

use std::collections::BTreeMap;

use alien_aws_clients::{AwsClientConfig, AwsCredentials};
use alien_azure_clients::{AzureClientConfig, AzureCredentials};
use alien_core::ClientConfig;
use alien_gcp_clients::{GcpClientConfig, GcpClientConfigExt};
use serde::{Deserialize, Serialize};

/// Errors the translator can raise. Callers wrap into their own error type
/// (managerx wraps into `ErrorData::CredentialResolutionFailed` /
/// `FeatureNotSupported`; the agent wraps into the commands-loop error).
#[derive(Debug, thiserror::Error)]
pub enum DebugSessionError {
    /// A credential variant we don't know how to project onto a user shell.
    /// The resolver path normally produces exportable variants — hitting this
    /// means an upstream change.
    #[error("alien debug ({platform}): {message}")]
    UnsupportedCredential { platform: String, message: String },

    /// The deployment's platform doesn't have a push-mode shell session
    /// (Kubernetes, Local, Test).
    #[error("alien debug: {message}")]
    UnsupportedPlatform { message: String },

    /// I/O error reading a credential off the manager's / agent's filesystem
    /// (e.g. the Azure federated-token file).
    #[error("alien debug ({platform}): {message}")]
    IoError { platform: String, message: String },

    /// Failed to mint a token from the resolved credential (e.g. GCP
    /// `IAMCredentials.generateAccessToken`).
    #[error("alien debug ({platform}): {message}")]
    TokenMintFailed { platform: String, message: String },
}

/// Wire response. Discriminated by `kind`. Identical shape on both ends so
/// the CLI can deserialize a session regardless of who produced it (manager
/// or agent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DebugSessionResponse {
    /// Cloud credentials projected as env vars (+ optional files / setup
    /// script). The CLI execs the user's command with the merged env.
    Push(PushDebugSession),
    /// Pure-Kubernetes session: a self-contained kubeconfig the CLI binds to
    /// `KUBECONFIG`. Used by pull-mode on-prem clusters where the agent mints
    /// a ServiceAccount token locally.
    Pull(PullDebugSession),
    /// Session creation is async. Returned for pull-mode Kubernetes deployments
    /// where the cluster is unreachable from the manager — the agent must first
    /// open an outbound tunnel back to the manager before the kubeconfig can
    /// resolve. The CLI long-polls `poll_url` until the session resolves to
    /// `Pull` (kubeconfig ready) or returns a 4xx/5xx with an error.
    Pending(PendingDebugSession),
    /// Push-mode cloud session via a manager-hosted WebSocket tunnel. Credentials
    /// stay on the manager; the CLI dials `tunnel_url`, spawns a local HTTP
    /// proxy bound to `AWS_ENDPOINT_URL` (or GCP/Azure equivalents), and
    /// every cloud-CLI request the child process emits is forwarded over the
    /// tunnel where the manager re-signs with the impersonated identity and
    /// proxies to the real cloud endpoint. Same security posture as pull-mode
    /// Kubernetes — nothing exportable leaves the manager.
    PushTunnel(PushTunnelDebugSession),
}

impl DebugSessionResponse {
    /// RFC3339 expiry of the underlying credentials, if the producer
    /// surfaced one. Used by the CLI's on-disk cache to skip
    /// near-expired sessions.
    pub fn expires_at(&self) -> Option<&str> {
        match self {
            Self::Push(p) => p.expires_at.as_deref(),
            Self::Pull(p) => p.expires_at.as_deref(),
            Self::Pending(_) => None,
            Self::PushTunnel(p) => p.expires_at.as_deref(),
        }
    }
}

/// Manager-hosted push-mode tunnel session. The CLI dials `tunnel_url` over
/// WebSocket, brings up a local HTTP proxy on a per-process port, sets
/// `<PROVIDER>_ENDPOINT_URL` env to that port, then execs the user's command.
/// The local proxy forwards every cloud-CLI HTTP request through the
/// WebSocket; the manager re-signs and proxies to the real cloud endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTunnelDebugSession {
    /// Server-assigned session id (`ds_…`). Embedded in audit, in URLs, in
    /// the tunnel handshake.
    pub session_id: String,
    /// `aws`, `gcp`, `azure`. Drives which `_ENDPOINT_URL` env var the CLI
    /// sets and which signing flow the manager runs.
    pub provider: String,
    /// Absolute `wss://…/v1/debug/sessions/<sid>/push-tunnel` URL.
    pub tunnel_url: String,
    /// Bearer the CLI presents on the WebSocket upgrade and on subsequent
    /// HTTP proxy requests; rotated server-side at session creation.
    pub client_token: String,
    /// RFC3339 expiry mirroring the underlying impersonated credential's TTL.
    /// CLI should refuse to use the tunnel past this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushDebugSession {
    /// Short identifier surfaced to the user (e.g. `"aws"`, `"gcp"`, `"azure"`).
    pub provider: String,
    /// Environment variables the CLI sets on the spawned process.
    pub env: BTreeMap<String, String>,
    /// Files the CLI materializes under the per-session tempdir before exec.
    /// When `env_var` is set, the CLI binds that env var to the resulting
    /// absolute file path.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<DebugCredFile>,
    /// Optional shell snippet the CLI runs (`sh -c`) after env/files are set
    /// up but before the user's command. Used today by Azure to drive
    /// `az login`. Must be idempotent — re-runs on every cache hit too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
    /// RFC3339 expiry. None when the credential type doesn't expose one
    /// (e.g. minted GCP access tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugCredFile {
    /// Filename — no path components. Written under the per-session tempdir.
    pub file_name: String,
    /// File contents.
    pub content: String,
    /// If set, the CLI binds this env var to the file's absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
}

/// Async-session handle. The CLI polls `poll_url` until the manager has
/// received the agent's tunnel-ready signal and can return a `Pull` payload
/// whose kubeconfig points at the per-session HTTPS proxy on the manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDebugSession {
    /// Server-assigned session id. Embedded in URLs and command channel
    /// messages so all parties (CLI, manager, agent) reference the same row.
    pub session_id: String,
    /// Absolute URL the CLI should GET to poll for readiness. Same auth as
    /// `POST /v1/debug/sessions`.
    pub poll_url: String,
    /// Suggested initial poll interval in milliseconds. The CLI should back
    /// off on repeated `pending` responses but never poll faster than this.
    #[serde(default)]
    pub poll_interval_ms: u32,
    /// RFC3339 absolute deadline. The CLI should give up after this and
    /// surface the most recent status. Bounded server-side; defaults to
    /// the session TTL.
    pub deadline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullDebugSession {
    /// Server-assigned session id. The CLI sends a DELETE to the manager on
    /// exit so the agent's `serve_session` ends and subsequent `alien debug`
    /// invocations don't wait for a 30-minute deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Kubeconfig YAML the CLI writes to a temp file and binds to
    /// `KUBECONFIG`.
    pub kubeconfig: String,
    /// Additional env vars the CLI sets alongside `KUBECONFIG`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Extra files to materialize alongside the kubeconfig.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<DebugCredFile>,
    /// When set, the CLI also spawns a local AWS loopback proxy and points
    /// `AWS_ENDPOINT_URL` at it. Cloud-CLI requests get tunnelled to the
    /// in-cluster operator, which signs with its IRSA identity and forwards
    /// to the real AWS endpoint. Populated for pull-mode K8s deployments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_endpoint_url: Option<String>,
    /// GCP equivalent — signed agent-side with the pod's GKE Workload
    /// Identity token. The CLI's loopback sets `CLOUDSDK_API_ENDPOINT_*`
    /// env vars to point gcloud / GCP SDKs at the loopback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gcp_endpoint_url: Option<String>,
    /// Azure equivalent — signed agent-side with the pod's Workload Identity
    /// federated token exchanged for an AAD bearer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azure_endpoint_url: Option<String>,
    /// Bearer the CLI's cloud loopbacks must present on requests to the
    /// `*_endpoint_url`s. Same `client_token` as the kubeconfig auth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_proxy_token: Option<String>,
    /// RFC3339 expiry. None when the SA token doesn't expose one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Translate a fully-resolved [`ClientConfig`] into a debug session payload.
///
/// Manager-side this runs against the impersonation-chain output; agent-side
/// it runs against `ClientConfig::from_std_env(platform)`. The translator
/// itself doesn't care which.
///
/// Kubernetes / KubernetesCloud / Local / Test all return
/// [`DebugSessionError::UnsupportedPlatform`] — pure-K8s pull sessions are
/// built by the agent directly (SA token + in-cluster CA) and returned as
/// [`DebugSessionResponse::Pull`] without going through this function.
pub async fn translate_client_config(
    config: ClientConfig,
) -> Result<DebugSessionResponse, DebugSessionError> {
    match config {
        ClientConfig::Aws(aws) => Ok(DebugSessionResponse::Push(translate_aws(*aws)?)),
        ClientConfig::Gcp(gcp) => Ok(DebugSessionResponse::Push(translate_gcp(*gcp).await?)),
        ClientConfig::Azure(azure) => Ok(DebugSessionResponse::Push(translate_azure(*azure)?)),
        ClientConfig::Kubernetes(_) | ClientConfig::KubernetesCloud { .. } => {
            Err(DebugSessionError::UnsupportedPlatform {
                message: "Kubernetes debug sessions don't go through translate_client_config; \
                          pull-mode K8s is handled agent-side by building a kubeconfig from the \
                          pod's ServiceAccount."
                    .to_string(),
            })
        }
        ClientConfig::Local { .. } | ClientConfig::Test => {
            Err(DebugSessionError::UnsupportedPlatform {
                message: "Local / test platforms do not support debug sessions".to_string(),
            })
        }
    }
}

fn translate_aws(cfg: AwsClientConfig) -> Result<PushDebugSession, DebugSessionError> {
    let mut env = BTreeMap::new();
    env.insert("AWS_REGION".to_string(), cfg.region.clone());
    env.insert("AWS_DEFAULT_REGION".to_string(), cfg.region.clone());

    let expires_at = match cfg.credentials {
        AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => {
            env.insert("AWS_ACCESS_KEY_ID".to_string(), access_key_id);
            env.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_access_key);
            env.insert("AWS_SESSION_TOKEN".to_string(), session_token);
            Some(expires_at)
        }
        AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => {
            env.insert("AWS_ACCESS_KEY_ID".to_string(), access_key_id);
            env.insert("AWS_SECRET_ACCESS_KEY".to_string(), secret_access_key);
            if let Some(token) = session_token {
                env.insert("AWS_SESSION_TOKEN".to_string(), token);
            }
            None
        }
        // The resolver path produces SessionCredentials via STS AssumeRole, so
        // hitting these variants means an upstream change. Fail loudly rather
        // than emit creds we don't actually understand.
        AwsCredentials::Imds { .. }
        | AwsCredentials::Profile { .. }
        | AwsCredentials::WebIdentity { .. } => {
            return Err(DebugSessionError::UnsupportedCredential {
                platform: "aws".to_string(),
                message: "Resolved AWS credentials are not directly exportable to a shell \
                          (got IMDS / Profile / WebIdentity)."
                    .to_string(),
            });
        }
    };

    Ok(PushDebugSession {
        provider: "aws".to_string(),
        env,
        files: Vec::new(),
        setup_script: None,
        expires_at,
    })
}

async fn translate_gcp(cfg: GcpClientConfig) -> Result<PushDebugSession, DebugSessionError> {
    // Mint a real access token regardless of underlying credential variant.
    // For impersonated service accounts (resolver default) this calls
    // IAMCredentials.generateAccessToken; for ServiceAccountKey it self-signs
    // a JWT and exchanges it; for AccessToken it returns the token as-is.
    let token =
        cfg.get_bearer_token("")
            .await
            .map_err(|e| DebugSessionError::TokenMintFailed {
                platform: "gcp".to_string(),
                message: format!(
                    "Failed to mint a GCP access token from the resolved credential: {e}"
                ),
            })?;

    let mut env = BTreeMap::new();
    // gcloud reads this for the authenticated identity instead of cached creds.
    env.insert("CLOUDSDK_AUTH_ACCESS_TOKEN".to_string(), token);
    env.insert("CLOUDSDK_CORE_PROJECT".to_string(), cfg.project_id);
    env.insert("CLOUDSDK_COMPUTE_REGION".to_string(), cfg.region);

    Ok(PushDebugSession {
        provider: "gcp".to_string(),
        env,
        files: Vec::new(),
        setup_script: None,
        // GCP doesn't surface an explicit expiry on `get_bearer_token`; tokens
        // are typically valid for ~1 hour. Surfacing a fake expiry would be
        // worse than no value.
        expires_at: None,
    })
}

/// Translate a resolved [`AzureClientConfig`] into a push session of env vars
/// (and, for federated workload identity, a token file + login script).
///
/// `DefaultAzureCredential` / `EnvironmentCredential` / `WorkloadIdentityCredential`
/// in every supported Azure SDK pick these env vars up automatically. For the
/// `az` CLI itself, the `setup_script` runs `az login` once before the user's
/// command — SDKs and `az` then both work.
fn translate_azure(cfg: AzureClientConfig) -> Result<PushDebugSession, DebugSessionError> {
    let mut env = BTreeMap::new();
    let mut files = Vec::new();

    env.insert("AZURE_TENANT_ID".to_string(), cfg.tenant_id.clone());
    env.insert(
        "AZURE_SUBSCRIPTION_ID".to_string(),
        cfg.subscription_id.clone(),
    );
    if let Some(region) = cfg.region.as_deref() {
        // No single env var is honored universally; `AZURE_DEFAULTS_LOCATION`
        // is what `az configure --defaults location=…` writes and what the
        // SDK's `ARM_DEFAULT_LOCATION`/`AZURE_DEFAULTS_LOCATION` readers look
        // for. Set both for compatibility.
        env.insert("AZURE_DEFAULTS_LOCATION".to_string(), region.to_string());
        env.insert("ARM_DEFAULT_LOCATION".to_string(), region.to_string());
    }

    let setup_script: String;

    match cfg.credentials {
        AzureCredentials::ServicePrincipal {
            client_id,
            client_secret,
        } => {
            env.insert("AZURE_CLIENT_ID".to_string(), client_id);
            env.insert("AZURE_CLIENT_SECRET".to_string(), client_secret);
            setup_script = AZ_LOGIN_SCRIPT_SERVICE_PRINCIPAL.to_string();
        }
        AzureCredentials::WorkloadIdentity {
            client_id,
            tenant_id,
            federated_token_file,
            authority_host,
        } => {
            // The federated token file lives on the producer's filesystem
            // (manager for push, agent for pull). Read it now and project
            // onto the user's machine so the SDK's
            // `WorkloadIdentityCredential` flow works locally.
            let token = std::fs::read_to_string(&federated_token_file).map_err(|e| {
                DebugSessionError::IoError {
                    platform: "azure".to_string(),
                    message: format!(
                        "Failed to read Azure federated token file '{}': {}",
                        federated_token_file, e
                    ),
                }
            })?;
            env.insert("AZURE_CLIENT_ID".to_string(), client_id);
            // tenant_id on the credential takes precedence over the outer
            // config tenant for cross-tenant impersonation.
            env.insert("AZURE_TENANT_ID".to_string(), tenant_id);
            env.insert("AZURE_AUTHORITY_HOST".to_string(), authority_host);
            files.push(DebugCredFile {
                file_name: "azure-federated-token".to_string(),
                content: token,
                env_var: Some("AZURE_FEDERATED_TOKEN_FILE".to_string()),
            });
            setup_script = AZ_LOGIN_SCRIPT_WORKLOAD_IDENTITY.to_string();
        }
        AzureCredentials::AccessToken { .. }
        | AzureCredentials::VmManagedIdentity { .. }
        | AzureCredentials::ManagedIdentity { .. } => {
            return Err(DebugSessionError::UnsupportedCredential {
                platform: "azure".to_string(),
                message: "Resolved Azure credentials are not directly exportable to a shell \
                          (got AccessToken / VmManagedIdentity / ManagedIdentity). Expected \
                          ServicePrincipal or WorkloadIdentity from the resolver."
                    .to_string(),
            });
        }
    }

    Ok(PushDebugSession {
        provider: "azure".to_string(),
        env,
        files,
        setup_script: Some(setup_script),
        // Azure access tokens are typically 1h but the credential type
        // doesn't surface an explicit expiry here. Surfacing a fake value
        // would be worse than no value.
        expires_at: None,
    })
}

/// Login script for the `az` CLI when the resolver produced a service
/// principal (local E2E path). Run by the user's CLI via `sh -c "$script"`
/// before the user's command.
const AZ_LOGIN_SCRIPT_SERVICE_PRINCIPAL: &str = r#"#!/bin/sh
# Generated by alien debug. Logs the Azure CLI in using the resolved
# service principal credentials. Safe to re-run; `az logout` first if you
# want a clean state.
set -eu
: "${AZURE_CLIENT_ID:?missing — set by alien debug}"
: "${AZURE_CLIENT_SECRET:?missing — set by alien debug}"
: "${AZURE_TENANT_ID:?missing — set by alien debug}"
: "${AZURE_SUBSCRIPTION_ID:?missing — set by alien debug}"
az login --service-principal \
  --username "$AZURE_CLIENT_ID" \
  --password "$AZURE_CLIENT_SECRET" \
  --tenant "$AZURE_TENANT_ID" \
  --output none
az account set --subscription "$AZURE_SUBSCRIPTION_ID"
"#;

/// Login script for the `az` CLI when the resolver produced a federated
/// workload-identity token (production impersonation path).
const AZ_LOGIN_SCRIPT_WORKLOAD_IDENTITY: &str = r#"#!/bin/sh
# Generated by alien debug. Logs the Azure CLI in using the resolved
# workload-identity federated token. Token expires with the session.
set -eu
: "${AZURE_CLIENT_ID:?missing — set by alien debug}"
: "${AZURE_TENANT_ID:?missing — set by alien debug}"
: "${AZURE_FEDERATED_TOKEN_FILE:?missing — set by alien debug}"
: "${AZURE_SUBSCRIPTION_ID:?missing — set by alien debug}"
az login --service-principal \
  --username "$AZURE_CLIENT_ID" \
  --tenant "$AZURE_TENANT_ID" \
  --federated-token "$(cat "$AZURE_FEDERATED_TOKEN_FILE")" \
  --output none
az account set --subscription "$AZURE_SUBSCRIPTION_ID"
"#;
