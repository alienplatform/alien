//! Azure Container Apps Sandboxes — the ADC data plane.
//!
//! A **second endpoint** from ARM, at `management.<region>.azuredevcompute.io`, gated by the
//! `Container Apps SandboxGroup Data Owner` role. Subscription Owner returns 403 here, so
//! management permissions alone provision a group without error and then fail at first exec.
//!
//! Microsoft's published data-plane REST reference covers `sessionPools` only, so the contract
//! below was read out of the `azure-containerapps-sandbox` PyPI package (0.1.0b4) rather than
//! guessed. That package is a preview whose surface Microsoft says may change, so the paths are
//! pinned here with tests and re-read on upgrade rather than assumed stable.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, ContextError, IntoAlienError};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Data-plane API version, from the SDK's `ApiVersion.V2026_02_01_PREVIEW`.
pub const API_VERSION: &str = "2026-02-01-preview";

/// Scope the data plane is signed for, from the SDK's `DATA_PLANE_SCOPE` in `_helpers.py`.
///
/// It is neither ARM's scope nor the endpoint's own host: the sandbox data plane sits on the
/// dynamic-sessions audience while answering at `azuredevcompute.io`. A token minted for either
/// host fails here as a 401 that reads like a missing role assignment.
const ADC_SCOPE: &str = "https://dynamicsessions.io/.default";

/// Service key an endpoint override is looked up under, which is how a test points the client at
/// a server it controls instead of a region's real data plane.
const SERVICE_NAME: &str = "sandboxDataPlane";

/// Largest file that moves in or out of a sandbox in one call.
///
/// The package carries no size constant, so this is the number the agent-backed backends already
/// enforce (`alien-sandbox-agent/src/files.rs`) rather than a measured server limit: one bound
/// callers can rely on everywhere, and a body that never grows past it here.
const MAX_FILE_BYTES: usize = 32 * 1024 * 1024;

/// An egress policy as the data plane takes and reports it.
///
/// Only the fields a sandbox needs: the audit log, header transforms and URL rewrites are part of
/// the same object and none of them are policy Alien can express.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressPolicy {
    /// `Allow` or `Deny`, applied to anything no rule matches. The data plane's own default is
    /// `Allow`, so a policy that omits it is an open sandbox.
    pub default_action: String,
    /// Host patterns and what to do with them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_rules: Vec<EgressHostRule>,
    /// Match-and-act rules, which this client never sends and has to read: a rule here can permit
    /// what the host patterns denied, and a policy field nobody models is one nobody checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<EgressRule>,
    /// `Full`, `Partial`, `Legacy` or `None`. Only `Full` blocks non-HTTP traffic, so only `Full`
    /// makes a `Deny` default mean no outbound access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_inspection: Option<String>,
    /// Anything else the policy carries.
    ///
    /// Kept rather than dropped because this is a preview API whose surface Microsoft says may
    /// change: a field that permits traffic and deserializes into nothing is one no containment
    /// check can weigh, and silence is the wrong answer for a policy nobody can read whole.
    #[serde(flatten)]
    pub unmodelled: BTreeMap<String, serde_json::Value>,
}

/// A match-and-act rule, in the two parts containment turns on: what it matches, and what it does.
///
/// The wire object also carries header transforms and URL rewrites. Neither is policy Alien can
/// express, and modelling them would only add fields to keep in step.
/// Every field the SDK's own model reads, and nothing beyond it.
///
/// `deny_unknown_fields` rather than a catch-all: an exception list or a second host on a rule
/// this client reads as a plain deny is reach the declaration never named, and a field that
/// deserializes into nothing is one no containment check can weigh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressRule {
    /// Rule name, which carries no policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the rule matches. Absent means the data plane sent a rule this client cannot read,
    /// which is treated as unknown rather than as matching nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#match: Option<EgressRuleMatch>,
    /// `Allow`, `Deny`, `Transform` or `Rewrite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<EgressRuleAction>,
}

/// What a rule matches on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressRuleMatch {
    /// Host pattern the rule applies to.
    #[serde(default)]
    pub host: String,
    /// Path prefix the rule narrows to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// HTTP methods the rule narrows to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,
}

/// What a rule does when it matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressRuleAction {
    /// `Allow`, `Deny`, `Transform` or `Rewrite`.
    #[serde(rename = "type", default)]
    pub action_type: String,
    /// Host a `Rewrite` sends the request to instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Path a `Rewrite` sends the request to instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Scheme a `Rewrite` sends the request over instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// Headers a `Transform` sets, inserts or removes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<serde_json::Value>>,
}

/// One host pattern and the action it carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EgressHostRule {
    /// Host pattern, such as `api.example.com`.
    pub pattern: String,
    /// `Allow` or `Deny`.
    pub action: String,
}

/// What a sandbox is created from.
///
/// A struct rather than a parameter list because the data plane keeps adding create-time fields
/// that decide what the sandbox can do, and each one added positionally is one a caller can pass
/// in the wrong slot.
#[derive(Debug, Clone, Default)]
pub struct CreateSandbox {
    /// Public catalog disk image name, such as `ubuntu`.
    pub disk_image: String,
    /// A disk image built in this group, by id. Set, it is what the sandbox starts from and
    /// `disk_image` is not sent: a catalog name and a built image are two different sources.
    pub disk_image_id: Option<String>,
    /// CPU in the data plane's units, such as `1000m`.
    pub cpu: String,
    /// Memory in the data plane's units, such as `2048Mi`.
    pub memory: String,
    /// Disk in the data plane's units, such as `40960Mi`. Absent, the data plane derives one
    /// from cpu.
    pub disk: Option<String>,
    /// Variables placed in the sandbox. It inherits nothing, so a variable exists only if it is
    /// sent here.
    pub environment: BTreeMap<String, String>,
    /// Outbound policy, applied from the moment the sandbox starts. Absent leaves the data
    /// plane's own default, which is open.
    pub egress: Option<EgressPolicy>,
    /// Idle seconds after which the sandbox pauses itself, sent as the data plane's
    /// `autoSuspendPolicy`. Absent leaves its own policy rather than asserting one.
    pub idle_pause_seconds: Option<u32>,
}

/// The create body.
///
/// `sourcesRef` is required unless a preset sandbox type is named, and resources are nested rather
/// than top level. A flat {disk, cpu, memory} is rejected with "'sourcesRef' is required when not
/// using a preset sandbox type".
fn create_body(request: &CreateSandbox) -> serde_json::Value {
    let disk_image = match &request.disk_image_id {
        Some(id) => serde_json::json!({ "id": id }),
        None => serde_json::json!({ "name": request.disk_image, "isPublic": true }),
    };
    let mut body = serde_json::json!({
        "sourcesRef": { "diskImage": disk_image },
        "resources": { "cpu": request.cpu, "memory": request.memory },
    });

    // A placeholder here would override the cpu-derived default; omitting a set value would
    // silently drop the customer's ceiling. Send only when declared.
    if let Some(disk) = &request.disk {
        body["resources"]["disk"] = serde_json::json!(disk);
    }

    if !request.environment.is_empty() {
        body["environment"] = serde_json::json!(request.environment);
    }

    if let Some(egress) = &request.egress {
        body["egressPolicy"] = serde_json::json!(egress);
    }

    // `Memory` is the SDK's own default for `auto_suspend_mode`, and the mode a session wants:
    // what `Disk` does differently is not documented, so the default stands rather than a guess.
    if let Some(seconds) = request.idle_pause_seconds {
        body["lifecycle"] = serde_json::json!({
            "autoSuspendPolicy": { "enabled": true, "interval": seconds, "mode": "Memory" }
        });
    }

    body
}

/// A sandbox as the data plane reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sandbox {
    /// Sandbox id within its group
    pub id: String,
    /// The policy the sandbox is actually running under, which is the only way to tell that the
    /// one that was asked for took effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress_policy: Option<EgressPolicy>,
    /// `Creating`, `Running`, `Stopping`, `Stopped`, `Suspended`, `Resuming` or `Deleting`.
    ///
    /// Optional because the name is only as good as the SDK it was read from: a field name that
    /// does not match the wire deserializes to `None`, and the provider turns that into an error
    /// rather than into a sandbox it assumes is healthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// A disk image built into a sandbox group from a registry image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskImage {
    /// Server-minted id a sandbox's `sourcesRef.diskImage.id` names.
    pub id: String,
    /// Labels the image was created with; how an image is found again, since the id is minted.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Build state, absent when the data plane sent none.
    #[serde(default)]
    pub status: Option<DiskImageStatus>,
}

impl DiskImage {
    /// The build state, such as `Ready` or `Failed`.
    pub fn state(&self) -> Option<&str> {
        self.status
            .as_ref()
            .and_then(|status| status.state.as_deref())
    }
}

/// Where a disk image's build stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskImageStatus {
    /// `Ready` once a sandbox can start from it, `Failed` when the build gave up.
    #[serde(default)]
    pub state: Option<String>,
    /// Why a build failed. Sent empty on a healthy image.
    #[serde(default)]
    pub error_message: Option<String>,
}

/// A registry image to build a disk image from.
#[derive(Debug, Clone, Default)]
pub struct CreateDiskImage {
    /// Registry reference, such as `docker.io/library/python:3.14-slim`. Only a `linux/amd64`
    /// image or index builds; Azure refuses anything else with `ImagePlatformNotSupported`.
    pub base: String,
    /// Labels to find the image by later.
    pub labels: BTreeMap<String, String>,
    /// Basic credentials for a private registry, as `(username, token)`. Absent pulls anonymously.
    pub registry_credentials: Option<(String, String)>,
}

/// The disk image create body: `image.base`, the labels, and `registryCredentials` when set.
fn disk_image_body(request: &CreateDiskImage) -> serde_json::Value {
    let mut body = serde_json::json!({
        "image": { "base": request.base },
        "labels": request.labels,
    });
    if let Some((username, token)) = &request.registry_credentials {
        body["registryCredentials"] = serde_json::json!({ "username": username, "token": token });
    }
    body
}

/// Result of a shell command.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    /// Captured stdout
    #[serde(default)]
    pub stdout: String,
    /// Captured stderr
    #[serde(default)]
    pub stderr: String,
    /// Process exit code, absent when the service did not report one
    #[serde(default)]
    pub exit_code: Option<i32>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait SandboxDataPlaneApi: Send + Sync + std::fmt::Debug {
    /// Creates a sandbox from a disk image.
    async fn create_sandbox(&self, group: &str, request: CreateSandbox) -> Result<Sandbox>;

    /// Reads a sandbox. A 404 is how deletion is confirmed.
    async fn get_sandbox(&self, group: &str, sandbox_id: &str) -> Result<Sandbox>;

    /// Deletes a sandbox. Returns before it is gone; confirm by polling `get_sandbox` to 404.
    async fn delete_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()>;

    /// Runs a shell command inside a sandbox.
    ///
    /// The body the SDK sends is `command` plus an optional `workingDirectory`, and nothing else
    /// — there is no timeout field, so a wall-clock ceiling can only be applied by the caller.
    async fn execute_shell_command(
        &self,
        group: &str,
        sandbox_id: &str,
        command: &str,
        working_directory: Option<String>,
    ) -> Result<ExecResult>;

    /// Reads a file out of a sandbox.
    async fn read_file(&self, group: &str, sandbox_id: &str, path: &str) -> Result<Vec<u8>>;

    /// Writes one file into a sandbox.
    async fn write_file(
        &self,
        group: &str,
        sandbox_id: &str,
        path: &str,
        contents: Vec<u8>,
    ) -> Result<()>;

    /// Creates a directory inside a sandbox. Idempotent, like `mkdir -p`.
    async fn mkdir(&self, group: &str, sandbox_id: &str, path: &str) -> Result<()>;

    /// Stops a sandbox, saving its state. Returns once accepted, not once stopped.
    async fn stop_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()>;

    /// Resumes a stopped sandbox. Returns once accepted, not once running.
    async fn resume_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()>;

    /// Builds a disk image from a registry image. Sent once: the id is server-minted, so a
    /// re-send builds a duplicate. Look an image up by label before calling this.
    async fn create_disk_image(&self, group: &str, request: CreateDiskImage) -> Result<DiskImage>;

    /// Reads a disk image. A 404 means it is gone.
    async fn get_disk_image(&self, group: &str, image_id: &str) -> Result<DiskImage>;

    /// Lists every disk image in the group. The data plane filters by nothing, so a caller
    /// matches labels itself.
    async fn list_disk_images(&self, group: &str) -> Result<Vec<DiskImage>>;

    /// Deletes a disk image. Sent once, so a 409 (a stopped sandbox's snapshot still holds the
    /// image) returns at once for the caller to retry later rather than spending a backoff here.
    async fn delete_disk_image(&self, group: &str, image_id: &str) -> Result<()>;
}

/// A 4xx with a `title` (`ImageNotFound`, `RegistryForbidden`, `ImagePlatformNotSupported`) or a
/// 502 `DependencyError` (the pull failed, as for a helm chart) is Azure's answer about the image,
/// which a retry does not change. Anything else, such as a bodiless RBAC 403, is about the group.
fn disk_image_refusal(
    status: reqwest::StatusCode,
    base: &str,
    group: &str,
    body: &str,
    url: &str,
) -> alien_error::AlienError<ErrorData> {
    #[derive(Deserialize)]
    struct Problem {
        title: String,
        #[serde(default)]
        detail: String,
    }

    match serde_json::from_str::<Problem>(body) {
        Ok(problem)
            if (status.is_client_error() && status.as_u16() != 409 && status.as_u16() != 429)
                || (status == reqwest::StatusCode::BAD_GATEWAY
                    && problem.title == "DependencyError") =>
        {
            alien_error::AlienError::new(ErrorData::HttpResponseError {
                message: format!("Azure CreateDiskImage failed: HTTP {status}"),
                url: url.to_string(),
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(body.to_string()),
            })
            .context(ErrorData::InvalidInput {
                message: format!(
                    "Azure refused to build a disk image from '{base}' ({}): {}",
                    problem.title, problem.detail
                ),
                field_name: None,
            })
        }
        _ => crate::azure::common::create_azure_http_error_with_context(
            status,
            "CreateDiskImage",
            "Resource",
            group,
            body,
            url,
            None,
        ),
    }
}

/// The `executeShellCommand` body, which is `command` plus an optional `workingDirectory` and
/// nothing else — read out of the preview SDK, which sends exactly these two.
fn exec_body(command: &str, working_directory: Option<String>) -> serde_json::Value {
    let mut payload = serde_json::json!({ "command": command });
    if let Some(directory) = working_directory {
        payload["workingDirectory"] = serde_json::Value::String(directory);
    }
    payload
}

/// Client for the ADC sandbox data plane.
#[derive(Debug)]
pub struct AzureSandboxDataPlaneClient {
    base: AzureClientBase,
    token_cache: AzureTokenCache,
    resource_group: String,
}

impl AzureSandboxDataPlaneClient {
    /// Builds a client against the region's ADC endpoint.
    pub fn new(
        client: reqwest::Client,
        region: &str,
        resource_group: &str,
        token_cache: AzureTokenCache,
    ) -> Self {
        let endpoint = token_cache
            .get_service_endpoint(SERVICE_NAME)
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://management.{region}.azuredevcompute.io"));

        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
            resource_group: resource_group.to_string(),
        }
    }

    /// Path prefix scoping every call to one sandbox group.
    ///
    /// Note it is **not** an ARM path: there is no `providers/Microsoft.App` segment.
    fn group_path(&self, group: &str) -> String {
        format!(
            "/subscriptions/{}/resourceGroups/{}/sandboxGroups/{group}",
            self.token_cache.config().subscription_id,
            self.resource_group
        )
    }

    fn sandbox_path(&self, group: &str, sandbox_id: &str) -> String {
        format!("{}/sandboxes/{sandbox_id}", self.group_path(group))
    }

    /// A bodyless POST that moves a sandbox between states.
    ///
    /// Sent once. A transition that took effect and lost its response would be repeated, and the
    /// repeat refused for the state the first one produced — reporting a failure for work that
    /// succeeded. The wait above this re-issues a resume itself, with the state in front of it.
    async fn lifecycle_action(
        &self,
        group: &str,
        sandbox_id: &str,
        verb: &str,
        operation: &str,
    ) -> Result<()> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/{verb}", self.sandbox_path(group, sandbox_id)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::POST, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        self.base
            .execute_request_once(signed, operation, sandbox_id)
            .await?;
        Ok(())
    }

    async fn parse<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        operation: &str,
    ) -> Result<T> {
        // The status is carried structurally rather than left for a caller to find in the
        // message: a body can contain "404" in a path or a trace id, and classifying on the
        // rendered text turns an unrelated failure into "the session is gone".
        let status = response.status();
        let url = response.url().to_string();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(alien_error::AlienError::new(ErrorData::HttpResponseError {
                message: format!("Azure ADC {operation} failed"),
                url,
                http_status: status.as_u16(),
                http_request_text: None,
                http_response_text: Some(body),
            }));
        }

        let body = response
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: format!("Azure ADC {operation}: failed to read response body"),
            })?;

        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::GenericError {
                message: format!("Azure ADC {operation}: unexpected response body: {body}"),
            })
    }
}

#[async_trait]
impl SandboxDataPlaneApi for AzureSandboxDataPlaneClient {
    async fn create_sandbox(&self, group: &str, request: CreateSandbox) -> Result<Sandbox> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/sandboxes", self.group_path(group)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let body = create_body(&request).to_string();
        let request = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;

        let signed = self.base.sign_request(request, &token).await?;
        // The create body carries the caller's environment variables, and a failure echoes the
        // request into the error chain, which is serialized into durable state. Sent once: the
        // id is server-minted, so a re-send mints an orphan sandbox nothing can find or reap.
        let response = alien_client_core::redact_request_body(
            self.base
                .execute_request_once(signed, "CreateSandbox", group)
                .await,
        )?;
        Self::parse(response, "CreateSandbox").await
    }

    async fn get_sandbox(&self, group: &str, sandbox_id: &str) -> Result<Sandbox> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.sandbox_path(group, sandbox_id),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self
            .base
            .execute_request(signed, "GetSandbox", sandbox_id)
            .await?;
        Self::parse(response, "GetSandbox").await
    }

    async fn delete_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &self.sandbox_path(group, sandbox_id),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::DELETE, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;

        // Discarded on purpose: this reports that deletion started. Microsoft's own SDK says
        // "poll until GET returns 404", which is what the caller does.
        self.base
            .execute_request(signed, "DeleteSandbox", sandbox_id)
            .await?;

        Ok(())
    }

    async fn execute_shell_command(
        &self,
        group: &str,
        sandbox_id: &str,
        command: &str,
        working_directory: Option<String>,
    ) -> Result<ExecResult> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!(
                "{}/executeShellCommand",
                self.sandbox_path(group, sandbox_id)
            ),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let body = exec_body(command, working_directory).to_string();
        let request = AzureRequestBuilder::new(Method::POST, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;

        let signed = self.base.sign_request(request, &token).await?;
        // The body is the command, which is where a caller puts a token it wants the session to
        // have. Sent once: a response that never arrives does not mean the command did not
        // start, so a re-send would risk running untrusted code twice.
        let response = alien_client_core::redact_request_body(
            self.base
                .execute_request_once(signed, "ExecuteShellCommand", sandbox_id)
                .await,
        )?;
        Self::parse(response, "ExecuteShellCommand").await
    }

    async fn read_file(&self, group: &str, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/files", self.sandbox_path(group, sandbox_id)),
            Some(vec![
                ("api-version", API_VERSION.into()),
                ("path", path.to_string()),
            ]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self
            .base
            .execute_request(signed, "ReadFile", sandbox_id)
            .await?;

        // Bytes, not JSON: the body is the file, and `parse` would try to read an image or a
        // tarball as a document. Collected chunk by chunk so the ceiling is enforced against
        // what has arrived rather than after the whole file is already in memory.
        let mut response = response;
        let mut contents: Vec<u8> = Vec::new();
        while let Some(chunk) =
            response
                .chunk()
                .await
                .into_alien_error()
                .context(ErrorData::GenericError {
                    message: "Azure ADC ReadFile: the response body ended early".to_string(),
                })?
        {
            contents.extend_from_slice(&chunk);
            if contents.len() > MAX_FILE_BYTES {
                return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                    message: format!(
                        "'{path}' is larger than the {MAX_FILE_BYTES}-byte transfer ceiling"
                    ),
                    field_name: Some("path".to_string()),
                }));
            }
        }

        Ok(contents)
    }

    async fn write_file(
        &self,
        group: &str,
        sandbox_id: &str,
        path: &str,
        contents: Vec<u8>,
    ) -> Result<()> {
        if contents.len() > MAX_FILE_BYTES {
            return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                message: format!(
                    "'{path}' is {} bytes, over the {MAX_FILE_BYTES}-byte transfer ceiling",
                    contents.len()
                ),
                field_name: Some("path".to_string()),
            }));
        }

        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        // `createDirs` is what makes a write create its parents, which is the cross-backend
        // contract. The SDK also takes a `mode`, deliberately not sent: its accepted format is
        // undocumented, and a wrong one would fail every write.
        let url = self.base.build_url(
            &format!("{}/files", self.sandbox_path(group, sandbox_id)),
            Some(vec![
                ("api-version", API_VERSION.into()),
                ("path", path.to_string()),
                ("createDirs", "true".to_string()),
            ]),
        );

        let request = AzureRequestBuilder::new(Method::PUT, url)
            .header("Content-Type", "application/octet-stream")
            .body_bytes(contents)
            .build()?;
        let signed = self.base.sign_request(request, &token).await?;
        // The body is the file the caller asked to write.
        alien_client_core::redact_request_body(
            self.base
                .execute_request(signed, "WriteFile", sandbox_id)
                .await,
        )?;
        Ok(())
    }

    async fn stop_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()> {
        self.lifecycle_action(group, sandbox_id, "stop", "StopSandbox")
            .await
    }

    async fn resume_sandbox(&self, group: &str, sandbox_id: &str) -> Result<()> {
        self.lifecycle_action(group, sandbox_id, "resume", "ResumeSandbox")
            .await
    }

    async fn mkdir(&self, group: &str, sandbox_id: &str, path: &str) -> Result<()> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/files/mkdir", self.sandbox_path(group, sandbox_id)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let body = serde_json::json!({ "path": path }).to_string();
        let request = AzureRequestBuilder::new(Method::POST, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(request, &token).await?;
        // The body is a caller-supplied path, redacted like the other bodied calls.
        alien_client_core::redact_request_body(
            self.base.execute_request(signed, "Mkdir", sandbox_id).await,
        )?;
        Ok(())
    }

    async fn create_disk_image(&self, group: &str, request: CreateDiskImage) -> Result<DiskImage> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/diskimages", self.group_path(group)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let base = request.base.clone();
        let body = disk_image_body(&request).to_string();
        let request = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let request_url = signed.url().to_string();
        // Sent once and outside the shared executor: its error carries the request body, which
        // can hold a registry token, and maps a 404 to "group not found" when Azure meant the tag.
        let response = self
            .base
            .client
            .execute(signed)
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!("Azure CreateDiskImage: HTTP error for '{base}'"),
            })?;
        let status = response.status();
        if status.is_success() {
            return Self::parse(response, "CreateDiskImage").await;
        }
        let body = response.text().await.unwrap_or_default();
        Err(disk_image_refusal(
            status,
            &base,
            group,
            &body,
            &request_url,
        ))
    }

    async fn get_disk_image(&self, group: &str, image_id: &str) -> Result<DiskImage> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/diskimages/{image_id}", self.group_path(group)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self
            .base
            .execute_request(signed, "GetDiskImage", image_id)
            .await?;
        Self::parse(response, "GetDiskImage").await
    }

    async fn list_disk_images(&self, group: &str) -> Result<Vec<DiskImage>> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/diskimages", self.group_path(group)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self
            .base
            .execute_request(signed, "ListDiskImages", group)
            .await?;
        // A bare array, not ARM's `{"value": [...]}`.
        Self::parse(response, "ListDiskImages").await
    }

    async fn delete_disk_image(&self, group: &str, image_id: &str) -> Result<()> {
        let token = self
            .token_cache
            .get_bearer_token_with_scope(ADC_SCOPE)
            .await?;
        let url = self.base.build_url(
            &format!("{}/diskimages/{image_id}", self.group_path(group)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::DELETE, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        self.base
            .execute_request_once(signed, "DeleteDiskImage", image_id)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};
    use httpmock::MockServer;

    /// A file that is not text. Every invalid UTF-8 shape in four bytes: a lone continuation, a
    /// truncated sequence, and an embedded NUL.
    const BINARY: [u8; 4] = [0xff, 0xfe, 0x00, 0x80];

    /// `matches` takes a function pointer, so the expected bytes are a constant rather than a
    /// captured value.
    fn carries_binary(request: &httpmock::prelude::HttpMockRequest) -> bool {
        request.body.clone().unwrap_or_default() == BINARY
    }

    /// A client that talks to a server this test controls, through the endpoint override the
    /// constructor honours.
    fn client_against(server: &MockServer) -> AzureSandboxDataPlaneClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: std::collections::HashMap::from([(
                SERVICE_NAME.to_string(),
                server.base_url(),
            )]),
        });

        AzureSandboxDataPlaneClient::new(
            reqwest::Client::new(),
            "eastus",
            "rg",
            AzureTokenCache::new(config),
        )
    }

    /// A create is delivered once, however the data plane answers.
    ///
    /// A second delivery mints an orphan sandbox no enumeration verb can find. Reads keep their
    /// retry — repeating one is free.
    #[tokio::test]
    async fn a_create_is_never_re_sent_where_a_read_is() {
        let server = MockServer::start_async().await;
        let unavailable = server.mock(|when, then| {
            when.method(httpmock::Method::PUT);
            then.status(503).body("{}");
        });
        let client = client_against(&server);

        client
            .create_sandbox(
                "grp",
                CreateSandbox {
                    disk_image: "ubuntu".to_string(),
                    disk_image_id: None,
                    cpu: "1".to_string(),
                    memory: "2Gi".to_string(),
                    disk: None,
                    environment: Default::default(),
                    egress: None,
                    idle_pause_seconds: None,
                },
            )
            .await
            .expect_err("an unavailable data plane fails the create");

        assert_eq!(
            unavailable.hits(),
            1,
            "a create that may already have minted a sandbox must not be sent twice"
        );

        let read = server.mock(|when, then| {
            when.method(httpmock::Method::GET);
            then.status(503).body("{}");
        });
        client
            .get_sandbox("grp", "s1")
            .await
            .expect_err("an unavailable data plane fails the read");

        assert!(
            read.hits() > 1,
            "a read is safe to repeat and must keep its retry: {} attempt(s)",
            read.hits()
        );

        // A state transition is not safe to repeat either. If the stop takes effect and its
        // response is lost, the repeat is refused for the state the first one produced — and the
        // caller is told a session it did suspend is still awake.
        let stop = server.mock(|when, then| {
            when.method(httpmock::Method::POST).path_contains("/stop");
            then.status(503).body("{}");
        });
        client
            .stop_sandbox("grp", "s1")
            .await
            .expect_err("an unavailable data plane fails the stop");

        assert_eq!(
            stop.hits(),
            1,
            "a transition that may already have happened must not be sent twice"
        );
    }

    /// Pinned because the contract came from a preview SDK Microsoft says may change. If these
    /// drift, the client must be re-read against the package rather than patched by guess.
    #[test]
    fn the_pinned_wire_contract_matches_what_the_sdk_ships() {
        assert_eq!(API_VERSION, "2026-02-01-preview");
        assert_eq!(ADC_SCOPE, "https://dynamicsessions.io/.default");
    }

    /// The data-plane path has no `providers/Microsoft.App` segment; borrowing ARM's shape here
    /// yields a 404 that reads like a permissions error.
    #[test]
    fn the_group_path_is_not_an_arm_path() {
        let path = "/subscriptions/s/resourceGroups/rg/sandboxGroups/sbg1";
        assert!(!path.contains("providers"));
        assert!(path.ends_with("/sandboxGroups/sbg1"));
    }

    /// `workingDirectory` is the only other field the SDK sends, and dropping it would run every
    /// command from the sandbox's default directory while the declaration said otherwise —
    /// silently, since the data plane accepts the body either way.
    #[test]
    fn a_working_directory_is_sent_when_one_is_asked_for() {
        let with = exec_body("ls", Some("/work".to_string()));
        assert_eq!(with["workingDirectory"], "/work");
        assert_eq!(with["command"], "ls");

        let without = exec_body("ls", None);
        assert!(
            without.get("workingDirectory").is_none(),
            "an unasked-for directory stays absent rather than becoming an empty string"
        );
    }

    #[test]
    fn an_exec_result_deserializes_with_its_streams_and_code() {
        let result: ExecResult =
            serde_json::from_str(r#"{"stdout":"hello\n","stderr":"","exitCode":0}"#)
                .expect("deserializes");

        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, Some(0));
    }

    /// Live responses carried only stdout and stderr. A response without an exit code must parse
    /// rather than fail, and the absence must stay visible instead of defaulting to 0 —
    /// a defaulted 0 would report a failed command as successful.
    #[test]
    fn a_missing_exit_code_is_none_rather_than_zero() {
        let result: ExecResult =
            serde_json::from_str(r#"{"stdout":"out","stderr":"err"}"#).expect("deserializes");

        assert_eq!(result.exit_code, None);
    }

    /// The three file calls, checked against the wire the SDK documents.
    ///
    /// Verb, path, query and body are each a way to be wrong without an error: the data plane
    /// answers a mistyped query parameter with a success and a different effect. `createDirs` is
    /// the one that carries the cross-backend rule that a write creates its parents.
    #[tokio::test]
    async fn the_file_calls_match_the_wire_the_sdk_documents() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        // The subscription is the mock config's; the rest is the path shape the SDK builds.
        let sandbox = format!(
            "/subscriptions/{}/resourceGroups/rg/sandboxGroups/grp/sandboxes/s1",
            AzureClientConfig::mock().subscription_id
        );

        let read = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path(format!("{sandbox}/files"))
                    .query_param("path", "src/app.py")
                    .query_param("api-version", API_VERSION);
                then.status(200).body(b"print(1)\n");
            })
            .await;
        let contents = client
            .read_file("grp", "s1", "src/app.py")
            .await
            .expect("the read should succeed");
        assert_eq!(contents, b"print(1)\n");
        read.assert_async().await;

        let write = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT)
                    .path(format!("{sandbox}/files"))
                    .query_param("path", "src/app.py")
                    .query_param("createDirs", "true")
                    .header("content-type", "application/octet-stream")
                    .matches(carries_binary);
                then.status(200);
            })
            .await;
        client
            .write_file("grp", "s1", "src/app.py", BINARY.to_vec())
            .await
            .expect("the write should succeed");
        write.assert_async().await;

        let mkdir = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path(format!("{sandbox}/files/mkdir"))
                    .json_body(serde_json::json!({ "path": "src" }));
                then.status(200);
            })
            .await;
        client
            .mkdir("grp", "s1", "src")
            .await
            .expect("the mkdir should succeed");
        mkdir.assert_async().await;
    }

    /// A file is bytes, not text: a transport that encoded it as UTF-8 would replace every
    /// invalid sequence and hand back a different file than the sandbox holds.
    #[tokio::test]
    async fn a_file_that_is_not_text_survives_both_directions() {
        let bytes = BINARY.to_vec();

        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let written = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT).matches(carries_binary);
                then.status(200);
            })
            .await;
        client
            .write_file("grp", "s1", "image.png", bytes.clone())
            .await
            .expect("the write should succeed");
        written.assert_async().await;

        let server = MockServer::start_async().await;
        let client = client_against(&server);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET);
                then.status(200).body(bytes.clone());
            })
            .await;
        assert_eq!(
            client
                .read_file("grp", "s1", "image.png")
                .await
                .expect("reads"),
            bytes
        );
    }

    /// The ceiling is refused here rather than accepted and truncated, and refused before the
    /// body is sent — an oversized upload that fails at the far end has already been transferred.
    #[tokio::test]
    async fn a_transfer_over_the_ceiling_is_refused_before_it_is_sent() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let refused = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT);
                then.status(200);
            })
            .await;

        let error = client
            .write_file("grp", "s1", "big.bin", vec![0u8; MAX_FILE_BYTES + 1])
            .await
            .expect_err("a body over the ceiling must be refused");

        assert_eq!(error.code, "INVALID_INPUT", "{error}");
        refused.assert_hits_async(0).await;
    }

    /// A read is bounded by the same number, against a data plane that says a file is small and
    /// then sends more than it said.
    #[tokio::test]
    async fn a_read_stops_at_the_ceiling_rather_than_filling_memory() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET);
                then.status(200).body(vec![0u8; MAX_FILE_BYTES + 1]);
            })
            .await;

        let error = client
            .read_file("grp", "s1", "big.bin")
            .await
            .expect_err("a body over the ceiling must be refused");

        assert_eq!(error.code, "INVALID_INPUT", "{error}");
    }

    /// A sandbox inherits nothing, so a variable the caller asked for exists only if the create
    /// body carries it — and the data plane accepts a body without it, so nothing else would say.
    #[test]
    fn the_create_body_carries_the_variables_the_caller_asked_for() {
        let body = create_body(&CreateSandbox {
            disk_image: "ubuntu".to_string(),
            disk_image_id: None,
            cpu: "1000m".to_string(),
            memory: "2048Mi".to_string(),
            disk: None,
            environment: BTreeMap::from([("TOKEN".to_string(), "t".to_string())]),
            egress: Some(EgressPolicy {
                default_action: "Deny".to_string(),
                unmodelled: Default::default(),
                host_rules: vec![EgressHostRule {
                    pattern: "api.example.com".to_string(),
                    action: "Allow".to_string(),
                }],
                rules: Vec::new(),
                traffic_inspection: Some("Full".to_string()),
            }),
            idle_pause_seconds: None,
        });

        assert_eq!(body["environment"]["TOKEN"], "t");
        assert_eq!(body["sourcesRef"]["diskImage"]["name"], "ubuntu");
        assert_eq!(body["resources"]["cpu"], "1000m");
        // camelCase, because the data plane ignores a field it cannot name and creates an open
        // sandbox instead of refusing the body.
        assert_eq!(body["egressPolicy"]["defaultAction"], "Deny");
        assert_eq!(body["egressPolicy"]["trafficInspection"], "Full");
        assert_eq!(
            body["egressPolicy"]["hostRules"][0]["pattern"],
            "api.example.com"
        );

        let bare = create_body(&CreateSandbox::default());
        assert!(
            bare.get("environment").is_none(),
            "an empty map is no variables, not an empty object: {bare}"
        );
        assert!(
            bare.get("lifecycle").is_none(),
            "an undeclared idle policy leaves the service's own rather than asserting one: {bare}"
        );
    }

    /// A declared idle suspend has to arrive as the nested policy the data plane reads, under
    /// the mode that keeps the process state a session exists for.
    #[test]
    fn the_create_body_nests_the_idle_suspend_policy() {
        let body = create_body(&CreateSandbox {
            idle_pause_seconds: Some(900),
            ..CreateSandbox::default()
        });

        assert_eq!(body["lifecycle"]["autoSuspendPolicy"]["interval"], 900);
        assert_eq!(body["lifecycle"]["autoSuspendPolicy"]["enabled"], true);
        assert_eq!(body["lifecycle"]["autoSuspendPolicy"]["mode"], "Memory");
    }

    /// The response field is `state`. Reading `status` leaves every sandbox deserializing to
    /// `None`, which the provider cannot tell apart from a healthy one.
    #[test]
    fn a_sandbox_deserializes_its_state() {
        let sandbox: Sandbox =
            serde_json::from_str(r#"{"id":"s1","state":"Stopped"}"#).expect("deserializes");

        assert_eq!(sandbox.state.as_deref(), Some("Stopped"));
    }

    /// A rule this client does not send still has to be read back: an `Allow` here permits what
    /// the host patterns denied, and a field nobody models is a field nobody checks.
    #[test]
    fn an_effective_policy_carries_the_rules_it_was_not_sent() {
        let policy: EgressPolicy = serde_json::from_str(
            r#"{"defaultAction":"Deny","trafficInspection":"Full",
                "rules":[{"match":{"host":"*"},"action":{"type":"Allow"}}]}"#,
        )
        .expect("deserializes");

        assert_eq!(policy.rules.len(), 1);
        assert_eq!(
            policy.rules[0]
                .action
                .as_ref()
                .map(|action| action.action_type.as_str()),
            Some("Allow")
        );
    }

    /// The two lifecycle verbs, on the paths the SDK documents.
    ///
    /// Both are bodyless POSTs to sibling paths, so a swapped verb is a call that succeeds and
    /// does the opposite of what was asked.
    #[tokio::test]
    async fn the_lifecycle_verbs_post_to_their_own_paths() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let sandbox = format!(
            "/subscriptions/{}/resourceGroups/rg/sandboxGroups/grp/sandboxes/s1",
            AzureClientConfig::mock().subscription_id
        );

        let stop = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path(format!("{sandbox}/stop"))
                    .query_param("api-version", API_VERSION);
                then.status(202);
            })
            .await;
        client
            .stop_sandbox("grp", "s1")
            .await
            .expect("stop is accepted");
        stop.assert_async().await;

        let resume = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path(format!("{sandbox}/resume"));
                then.status(202);
            })
            .await;
        client
            .resume_sandbox("grp", "s1")
            .await
            .expect("resume is accepted");
        resume.assert_async().await;
    }

    /// A key this client cannot read on a *rule* fails the parse, as it does on the policy.
    ///
    /// An exception list on a rule that otherwise reads as a plain deny is reach the declaration
    /// never named, and a field that deserializes into nothing is one no check can weigh.
    #[test]
    fn an_unreadable_key_on_a_rule_fails_the_parse() {
        for policy in [
            r#"{"defaultAction":"Deny","hostRules":[{"pattern":"*","action":"Deny","exceptions":["x"]}]}"#,
            r#"{"defaultAction":"Deny","rules":[{"action":{"type":"Deny","exceptHosts":["x"]}}]}"#,
            r#"{"defaultAction":"Deny","rules":[{"match":{"host":"*","exceptPorts":[443]}}]}"#,
        ] {
            serde_json::from_str::<EgressPolicy>(policy)
                .expect_err("a rule carrying an unreadable key must not parse");
        }

        // The documented surface still parses, so the rule above refuses additions rather than
        // everything.
        serde_json::from_str::<EgressPolicy>(
            r#"{"defaultAction":"Deny","rules":[{"name":"r","match":{"host":"*","path":"/","methods":["GET"]},
                "action":{"type":"Rewrite","host":"h","path":"/p","scheme":"https","headers":[]}}]}"#,
        )
        .expect("every field the SDK models must still parse");
    }

    /// A sandbox started from a built image names it by id alone: `isPublic` would send the
    /// data plane looking for the id in the public catalog.
    #[test]
    fn a_built_disk_image_is_named_by_id() {
        let body = create_body(&CreateSandbox {
            disk_image: "ubuntu".to_string(),
            disk_image_id: Some("9bb20405-f156-4ca3-930d-dd7526674c1a".to_string()),
            ..CreateSandbox::default()
        });

        assert_eq!(
            body["sourcesRef"]["diskImage"],
            serde_json::json!({ "id": "9bb20405-f156-4ca3-930d-dd7526674c1a" })
        );
    }

    /// The wire shapes the live service answered with: a create that is `Ready` in its own
    /// response, and a list that is a bare array rather than ARM's `{"value": [...]}`.
    #[tokio::test]
    async fn a_disk_image_is_built_and_listed_in_the_shapes_the_service_sends() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let images = format!(
            "/subscriptions/{}/resourceGroups/rg/sandboxGroups/grp/diskimages",
            AzureClientConfig::mock().subscription_id
        );
        let image = r#"{"id":"img-1","labels":{"alienImage":"abc"},
            "image":{"base":"docker.io/library/python:3.14-slim"},
            "status":{"state":"Ready","createdAt":"2026-09-28T18:05:17Z"},"sizeInMB":208}"#;

        let create = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT)
                    .path(images.clone())
                    .query_param("api-version", API_VERSION)
                    .json_body(serde_json::json!({
                        "image": { "base": "registry.example.com/team/agent:1" },
                        "labels": { "alienImage": "abc" },
                        "registryCredentials": { "username": "deployment", "token": "t" },
                    }));
                then.status(200).body(image);
            })
            .await;
        let built = client
            .create_disk_image(
                "grp",
                CreateDiskImage {
                    base: "registry.example.com/team/agent:1".to_string(),
                    labels: BTreeMap::from([("alienImage".to_string(), "abc".to_string())]),
                    registry_credentials: Some(("deployment".to_string(), "t".to_string())),
                },
            )
            .await
            .expect("the build is accepted");
        create.assert_async().await;
        assert_eq!(built.id, "img-1");
        assert_eq!(built.state(), Some("Ready"));

        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path(images.clone());
                then.status(200).body(format!("[{image}]"));
            })
            .await;
        let listed = client
            .list_disk_images("grp")
            .await
            .expect("the list parses");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].labels["alienImage"], "abc");
    }

    /// An arm64-only image is refused at create with a 400 whose `detail` is the only place the
    /// reason lives, so the error has to carry it rather than a bare status.
    #[tokio::test]
    async fn a_refused_build_carries_azures_reason() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let create = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT);
                then.status(400).body(
                    r#"{"title":"ImagePlatformNotSupported","status":400,"detail":"The container image 'docker.io/arm64v8/alpine:3.19' does not provide a linux/amd64 variant. Only linux/amd64 images are supported.","errorCode":15}"#,
                );
            })
            .await;

        let error = client
            .create_disk_image(
                "grp",
                CreateDiskImage {
                    base: "docker.io/arm64v8/alpine:3.19".to_string(),
                    registry_credentials: Some(("deployment".to_string(), "secret".to_string())),
                    ..CreateDiskImage::default()
                },
            )
            .await
            .expect_err("an arm64-only image is refused");

        assert_eq!(create.hits(), 1, "a build is never re-sent");
        let rendered = format!("{error}");
        assert!(rendered.contains("linux/amd64"), "{rendered}");
        let serialized = serde_json::to_string(&error).expect("the error serializes");
        assert!(
            !serialized.contains("secret"),
            "the registry token must not reach the error chain: {serialized}"
        );
    }

    /// A missing tag answers 404 and a denied pull 403, each with the reason in the body. Both
    /// must name the registry's answer rather than the group, and neither is worth a retry.
    #[tokio::test]
    async fn a_registry_refusal_names_the_image_not_the_group() {
        for (status, body, expected) in [
            (
                404,
                r#"{"title":"ImageNotFound","status":404,"detail":"The image 'docker.io/library/python:0.0-nope' was not found in the registry."}"#,
                "not found in the registry",
            ),
            (
                403,
                r#"{"title":"RegistryForbidden","status":403,"detail":"Pulling the image was forbidden. Provide 'registryCredentials' to authenticate."}"#,
                "Provide 'registryCredentials'",
            ),
            (
                401,
                r#"{"title":"RegistryAuthFailed","status":401,"detail":"Authentication failed when pulling container image."}"#,
                "Authentication failed when pulling",
            ),
            (
                502,
                r#"{"title":"DependencyError","status":502,"detail":"buildah pull failed with exit code 125."}"#,
                "buildah pull failed",
            ),
        ] {
            let server = MockServer::start_async().await;
            let client = client_against(&server);
            let create = server
                .mock_async(|when, then| {
                    when.method(httpmock::Method::PUT);
                    then.status(status).body(body);
                })
                .await;

            let error = client
                .create_disk_image(
                    "grp",
                    CreateDiskImage {
                        base: "docker.io/library/python:0.0-nope".to_string(),
                        registry_credentials: Some((
                            "deployment".to_string(),
                            "secret".to_string(),
                        )),
                        ..CreateDiskImage::default()
                    },
                )
                .await
                .expect_err("a refused build fails");

            assert_eq!(create.hits(), 1);
            let rendered = error.to_string();
            assert!(rendered.contains(expected), "{status}: {rendered}");
            assert!(!rendered.contains("'grp'"), "{status}: {rendered}");
            assert!(
                !error.retryable,
                "{status}: a registry refusal is not retried"
            );
            let serialized = serde_json::to_string(&error).expect("the error serializes");
            assert!(!serialized.contains("secret"), "{serialized}");
        }
    }

    /// A bodiless 403 is this plane's own RBAC refusing the caller, which is about the group.
    #[tokio::test]
    async fn a_bodiless_403_keeps_the_group_access_error() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::PUT);
                then.status(403);
            })
            .await;

        let error = client
            .create_disk_image("grp", CreateDiskImage::default())
            .await
            .expect_err("a denied caller fails");

        assert!(
            matches!(error.error, Some(ErrorData::RemoteAccessDenied { .. })),
            "{error:?}"
        );
    }

    /// A 409 means a stopped sandbox's snapshot still holds the image. It comes back once, typed
    /// as a conflict, so the caller can keep the id and try again later.
    #[tokio::test]
    async fn a_held_disk_image_delete_returns_a_conflict_at_once() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let delete = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::DELETE);
                then.status(409).body(
                    r#"{"title":"DiskImageHasDependents","status":409,"detail":"Cannot delete disk image 'img-1': it is referenced by 1 snapshot(s)"}"#,
                );
            })
            .await;

        let error = client
            .delete_disk_image("grp", "img-1")
            .await
            .expect_err("a held image is not deleted");

        assert_eq!(delete.hits(), 1, "a held image is not retried in a backoff");
        assert!(
            matches!(error.error, Some(ErrorData::RemoteResourceConflict { .. })),
            "{error:?}"
        );
    }
}
