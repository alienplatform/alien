//! Wire shapes for `alien debug` sessions.
//!
//! These types are the contract between the CLI, the manager (push mode),
//! and the agent (pull mode). Pure data + a small AWS-host parser; no
//! cloud-client dependencies, so any layer in the dep graph can speak them.
//!
//! The credential *translator* — projecting a resolved [`crate::ClientConfig`]
//! onto a `DebugSessionResponse::Push` payload — lives in `alien-platform-core`,
//! one tier above the cloud-client crates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Errors raised by the debug-session producer (manager or agent). Callers
/// wrap into their own error type when surfacing across crate boundaries.
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
    /// `KUBECONFIG`.
    Pull(PullDebugSession),
    /// Session creation is async. Returned for pull-mode deployments where
    /// the agent must first open an outbound tunnel back to the manager
    /// before the kubeconfig resolves. The CLI long-polls `poll_url` until
    /// the session resolves to `Pull` (kubeconfig ready) or errors out.
    Pending(PendingDebugSession),
    /// Push-mode cloud session via a manager-hosted WebSocket tunnel.
    /// Credentials stay on the manager; the CLI dials `tunnel_url`, spawns
    /// a local HTTP proxy, and every cloud-CLI request the child process
    /// emits is forwarded over the tunnel for the manager to re-sign with
    /// the impersonated identity.
    PushTunnel(PushTunnelDebugSession),
    /// Runtime shell/exec session via an agent-hosted process tunnel.
    RuntimeTunnel(RuntimeTunnelDebugSession),
}

impl DebugSessionResponse {
    /// RFC3339 expiry of the underlying credentials, if the producer
    /// surfaced one. Used by the CLI's on-disk cache to skip near-expired
    /// sessions.
    pub fn expires_at(&self) -> Option<&str> {
        match self {
            Self::Push(p) => p.expires_at.as_deref(),
            Self::Pull(p) => p.expires_at.as_deref(),
            Self::Pending(_) => None,
            Self::PushTunnel(p) => p.expires_at.as_deref(),
            Self::RuntimeTunnel(p) => p.expires_at.as_deref(),
        }
    }
}

/// What kind of debug session the caller is requesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DebugSessionKind {
    /// Existing behavior: run local commands with deployment context.
    Context,
    /// Open an interactive shell in the deployment runtime.
    RuntimeShell,
    /// Run one non-interactive command in the deployment runtime.
    RuntimeExec,
}

impl Default for DebugSessionKind {
    fn default() -> Self {
        Self::Context
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeTunnelDebugSession {
    /// Server-assigned session id.
    pub session_id: String,
    /// `local` for v1.
    pub platform: String,
    /// Absolute `wss://…/v1/debug/sessions/<sid>/runtime-client` URL.
    pub tunnel_url: String,
    /// Bearer the CLI presents on the WebSocket upgrade.
    pub client_token: String,
    /// Runtime operation this tunnel accepts.
    pub kind: DebugSessionKind,
    /// Runtime frame protocol version.
    pub protocol_version: u32,
    /// RFC3339 expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Client-to-agent runtime debug frames, relayed by the manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuntimeClientFrame {
    /// Start an interactive shell with an optional initial terminal size.
    StartShell {
        /// Terminal columns.
        cols: u16,
        /// Terminal rows.
        rows: u16,
    },
    /// Start a non-interactive command.
    StartExec {
        /// Executable and argv to run on the remote host.
        command: Vec<String>,
        /// Optional timeout in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Standard input bytes, base64-encoded.
    Stdin {
        /// Base64-encoded bytes.
        data_b64: String,
    },
    /// Resize the interactive terminal.
    Resize {
        /// Terminal columns.
        cols: u16,
        /// Terminal rows.
        rows: u16,
    },
    /// Close stdin for the remote process.
    CloseStdin,
    /// Cancel the remote process.
    Cancel,
}

/// Agent-to-client runtime debug frames, relayed by the manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuntimeAgentFrame {
    /// The process has started.
    Started {
        /// Optional process id where available.
        pid: Option<u32>,
        /// Human-readable account or shell description.
        detail: Option<String>,
    },
    /// Standard output bytes, base64-encoded.
    Stdout {
        /// Base64-encoded bytes.
        data_b64: String,
    },
    /// Standard error bytes, base64-encoded.
    Stderr {
        /// Base64-encoded bytes.
        data_b64: String,
    },
    /// The process exited.
    Exit {
        /// Process exit code. `None` means signal/unknown.
        code: Option<i32>,
        /// Whether the process was terminated by the runtime timeout.
        timed_out: bool,
        /// Whether output was truncated.
        output_truncated: bool,
    },
    /// The remote runtime failed before producing an exit code.
    Error {
        /// Safe, user-facing error message.
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushTunnelDebugSession {
    /// Server-assigned session id (`ds_…`).
    pub session_id: String,
    /// `aws`, `gcp`, `azure`. Drives which `_ENDPOINT_URL` env var the CLI
    /// sets and which signing flow the manager runs.
    pub provider: String,
    /// Absolute `wss://…/v1/debug/sessions/<sid>/push-tunnel` URL.
    pub tunnel_url: String,
    /// Bearer the CLI presents on the WebSocket upgrade and on subsequent
    /// HTTP proxy requests.
    pub client_token: String,
    /// RFC3339 expiry mirroring the underlying impersonated credential's TTL.
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
    /// up but before the user's command. Must be idempotent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
    /// RFC3339 expiry. None when the credential type doesn't expose one.
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
    /// exit so the agent's `serve_session` ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Kubeconfig YAML the CLI writes to a temp file and binds to `KUBECONFIG`.
    pub kubeconfig: String,
    /// Additional env vars the CLI sets alongside `KUBECONFIG`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Extra files to materialize alongside the kubeconfig.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<DebugCredFile>,
    /// When set, the CLI also spawns a local AWS loopback proxy and points
    /// `AWS_ENDPOINT_URL` at it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_endpoint_url: Option<String>,
    /// GCP equivalent — signed agent-side with the pod's GKE Workload Identity
    /// token.
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

/// Derive `(service, signing_region)` from an AWS API URL host.
///
/// Handles the common shapes:
///
/// - `<service>.<region>.amazonaws.com`     → (service, region)
/// - `<service>.amazonaws.com`              → (service, fallback_region)
/// - `<bucket>.s3.<region>.amazonaws.com`   → ("s3", region)
/// - `<bucket>.s3.amazonaws.com`            → ("s3", fallback_region)
///
/// Falls back to `fallback_region` when the host doesn't carry one. Takes
/// `&str` so this stays HTTP-client-agnostic.
pub fn extract_aws_service_and_region(host: &str, fallback_region: &str) -> (&'static str, String) {
    let labels: Vec<&str> = host.split('.').collect();

    let Some(amz_idx) = labels.iter().rposition(|l| *l == "amazonaws") else {
        return ("execute-api", fallback_region.to_string());
    };

    let (service, region) = match &labels[..amz_idx] {
        [_bucket_or_subdomain @ .., service, region]
            if region.contains('-') && service.len() <= 8 =>
        {
            (*service, region.to_string())
        }
        [_subdomain @ .., service] => (*service, fallback_region.to_string()),
        _ => ("execute-api", fallback_region.to_string()),
    };

    let static_service: &'static str = match service {
        "sts" => "sts",
        "iam" => "iam",
        "ec2" => "ec2",
        "lambda" => "lambda",
        "s3" => "s3",
        "dynamodb" => "dynamodb",
        "sqs" => "sqs",
        "sns" => "sns",
        "ecr" => "ecr",
        "eks" => "eks",
        "ecs" => "ecs",
        "cloudformation" => "cloudformation",
        "cloudwatch" => "monitoring",
        "logs" => "logs",
        "ssm" => "ssm",
        "secretsmanager" => "secretsmanager",
        "kms" => "kms",
        "events" | "eventbridge" => "events",
        "apigateway" => "apigateway",
        "execute-api" => "execute-api",
        _ => "execute-api",
    };

    (static_service, region)
}

#[cfg(test)]
mod aws_endpoint_parsing_tests {
    use super::extract_aws_service_and_region;

    #[test]
    fn regional_service() {
        assert_eq!(
            extract_aws_service_and_region("ec2.us-east-1.amazonaws.com", "us-west-2"),
            ("ec2", "us-east-1".to_string())
        );
    }

    #[test]
    fn global_service() {
        assert_eq!(
            extract_aws_service_and_region("iam.amazonaws.com", "us-east-1"),
            ("iam", "us-east-1".to_string())
        );
    }

    #[test]
    fn s3_bucket_regional() {
        assert_eq!(
            extract_aws_service_and_region("mybucket.s3.us-east-1.amazonaws.com", "us-west-2"),
            ("s3", "us-east-1".to_string())
        );
    }

    #[test]
    fn unknown_host() {
        assert_eq!(
            extract_aws_service_and_region("internal.example.com", "us-east-1"),
            ("execute-api", "us-east-1".to_string())
        );
    }
}
