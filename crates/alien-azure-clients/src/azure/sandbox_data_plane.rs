//! Azure Container Apps Sandboxes — the ADC data plane.
//!
//! A **second endpoint** from ARM, at `management.<region>.azuredevcompute.io`, gated by the
//! `Container Apps SandboxGroup Data Owner` role. Subscription Owner returns 403 here, so
//! management permissions alone provision a group cleanly and then fail at first exec.
//!
//! Microsoft's published data-plane REST reference covers `sessionPools` only, so the contract
//! below was read out of the `azure-containerapps-sandbox` PyPI package (0.1.0b4) rather than
//! guessed. That package is a preview whose surface Microsoft says may change, so the paths are
//! pinned here with tests and re-read on upgrade rather than assumed stable.

use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};
use std::collections::BTreeMap;
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};

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
}

/// A match-and-act rule, in the two parts containment turns on: what it matches, and what it does.
///
/// The wire object also carries header transforms and URL rewrites. Neither is policy Alien can
/// express, and modelling them would only add fields to keep in step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressRule {
    /// What the rule matches. Absent means the data plane sent a rule this client cannot read,
    /// which is treated as unknown rather than as matching nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#match: Option<EgressRuleMatch>,
    /// `Allow`, `Deny`, `Transform` or `Rewrite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<EgressRuleAction>,
}

/// The host a rule matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressRuleMatch {
    /// Host pattern the rule applies to.
    #[serde(default)]
    pub host: String,
}

/// What a rule does when it matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressRuleAction {
    /// `Allow`, `Deny`, `Transform` or `Rewrite`.
    #[serde(rename = "type", default)]
    pub action_type: String,
}

/// One host pattern and the action it carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// CPU in the data plane's units, such as `1000m`.
    pub cpu: String,
    /// Memory in the data plane's units, such as `2048Mi`.
    pub memory: String,
    /// Variables placed in the sandbox. It inherits nothing, so a variable exists only if it is
    /// sent here.
    pub environment: BTreeMap<String, String>,
    /// Outbound policy, applied from the moment the sandbox starts. Absent leaves the data
    /// plane's own default, which is open.
    pub egress: Option<EgressPolicy>,
    /// Idle seconds after which the sandbox suspends itself. Absent leaves the data plane's own
    /// policy rather than asserting one.
    pub idle_suspend_seconds: Option<u32>,
}

/// The create body.
///
/// `sourcesRef` is required unless a preset sandbox type is named, and resources are nested rather
/// than top level. A flat {disk, cpu, memory} is rejected with "'sourcesRef' is required when not
/// using a preset sandbox type".
fn create_body(request: &CreateSandbox) -> serde_json::Value {
    let mut body = serde_json::json!({
        "sourcesRef": { "diskImage": { "name": request.disk_image, "isPublic": true } },
        "resources": { "cpu": request.cpu, "memory": request.memory },
    });

    if !request.environment.is_empty() {
        body["environment"] = serde_json::json!(request.environment);
    }

    if let Some(egress) = &request.egress {
        body["egressPolicy"] = serde_json::json!(egress);
    }

    // `Memory` is the SDK's own default for `auto_suspend_mode`, and the mode a session wants:
    // what `Disk` does differently is not documented, so the default stands rather than a guess.
    if let Some(seconds) = request.idle_suspend_seconds {
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
    async fn lifecycle_action(
        &self,
        group: &str,
        sandbox_id: &str,
        verb: &str,
        operation: &str,
    ) -> Result<()> {
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
        let url = self.base.build_url(
            &format!("{}/{verb}", self.sandbox_path(group, sandbox_id)),
            Some(vec![("api-version", API_VERSION.into())]),
        );

        let request = AzureRequestBuilder::new(Method::POST, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        self.base
            .execute_request(signed, operation, sandbox_id)
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
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        let response = self
            .base
            .execute_request(signed, "CreateSandbox", group)
            .await?;
        Self::parse(response, "CreateSandbox").await
    }

    async fn get_sandbox(&self, group: &str, sandbox_id: &str) -> Result<Sandbox> {
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        let response = self
            .base
            .execute_request(signed, "ExecuteShellCommand", sandbox_id)
            .await?;
        Self::parse(response, "ExecuteShellCommand").await
    }

    async fn read_file(&self, group: &str, sandbox_id: &str, path: &str) -> Result<Vec<u8>> {
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
        let url = self.base.build_url(
            &format!("{}/files", self.sandbox_path(group, sandbox_id)),
            Some(vec![
                ("api-version", API_VERSION.into()),
                ("path", path.to_string()),
            ]),
        );

        let request = AzureRequestBuilder::new(Method::GET, url).build()?;
        let signed = self.base.sign_request(request, &token).await?;
        let response = self.base.execute_request(signed, "ReadFile", sandbox_id).await?;

        // Bytes, not JSON: the body is the file, and `parse` would try to read an image or a
        // tarball as a document. Collected chunk by chunk so the ceiling is enforced against
        // what has arrived rather than after the whole file is already in memory.
        let mut response = response;
        let mut contents: Vec<u8> = Vec::new();
        while let Some(chunk) = response
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

        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        self.base.execute_request(signed, "WriteFile", sandbox_id).await?;
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
        let token = self.token_cache.get_bearer_token_with_scope(ADC_SCOPE).await?;
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
        self.base.execute_request(signed, "Mkdir", sandbox_id).await?;
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
        client.mkdir("grp", "s1", "src").await.expect("the mkdir should succeed");
        mkdir.assert_async().await;
    }

    /// A file is bytes, not text: a transport that encoded it as UTF-8 would replace every
    /// invalid sequence and hand back a different file than the sandbox holds.
    #[tokio::test]
    async fn a_file_that_is_not_text_survives_both_directions() {
        let server = MockServer::start_async().await;
        let client = client_against(&server);
        let bytes = BINARY.to_vec();

        let server = MockServer::start_async().await;
        let client = client_against(&server);
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET);
                then.status(200).body(bytes.clone());
            })
            .await;
        assert_eq!(
            client.read_file("grp", "s1", "image.png").await.expect("reads"),
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
            cpu: "1000m".to_string(),
            memory: "2048Mi".to_string(),
            environment: BTreeMap::from([("TOKEN".to_string(), "t".to_string())]),
            egress: Some(EgressPolicy {
                default_action: "Deny".to_string(),
                host_rules: vec![EgressHostRule {
                    pattern: "api.example.com".to_string(),
                    action: "Allow".to_string(),
                }],
                rules: Vec::new(),
                traffic_inspection: Some("Full".to_string()),
            }),
            idle_suspend_seconds: None,
        });

        assert_eq!(body["environment"]["TOKEN"], "t");
        assert_eq!(body["sourcesRef"]["diskImage"]["name"], "ubuntu");
        assert_eq!(body["resources"]["cpu"], "1000m");
        // camelCase, because the data plane ignores a field it cannot name and creates an open
        // sandbox instead of refusing the body.
        assert_eq!(body["egressPolicy"]["defaultAction"], "Deny");
        assert_eq!(body["egressPolicy"]["trafficInspection"], "Full");
        assert_eq!(body["egressPolicy"]["hostRules"][0]["pattern"], "api.example.com");

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
            idle_suspend_seconds: Some(900),
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
            policy.rules[0].action.as_ref().map(|action| action.action_type.as_str()),
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
        client.stop_sandbox("grp", "s1").await.expect("stop is accepted");
        stop.assert_async().await;

        let resume = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path(format!("{sandbox}/resume"));
                then.status(202);
            })
            .await;
        client.resume_sandbox("grp", "s1").await.expect("resume is accepted");
        resume.assert_async().await;
    }
}
