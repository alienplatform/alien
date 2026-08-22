//! AWS Lambda MicroVMs client.
//!
//! Hand-rolled rather than SDK-backed: there is no Rust SDK for this API. The wire contract
//! below was read out of `@aws-sdk/client-lambda-microvms`, the published JS SDK,
//! rather than guessed, since an invented path fails as a 404 that looks like a permissions
//! problem.
//!
//! Signed as `lambda`, and the tagging operations sit on Lambda's own `/2017-03-31/tags` path.

use crate::aws::aws_request_utils::{sign_send_json, AwsSignConfig};
use crate::aws::credential_provider::AwsCredentialProvider;
use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// API version every MicroVM path is prefixed with.
const API_VERSION: &str = "2025-09-09";

/// Longest life AWS will mint an endpoint auth token for.
pub const MAX_AUTH_TOKEN_MINUTES: u32 = 60;

/// A MicroVM image, the Frozen parent of a sandbox's sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrovmImage {
    /// Image identifier used in every subsequent path
    pub image_identifier: Option<String>,
    /// Image ARN
    pub image_arn: Option<String>,
    /// Current version, which together with the image scopes session discovery.
    ///
    /// `GetMicrovmImage` names it `latestActiveImageVersion` while a version listing names it
    /// `imageVersion`, so both have to land here — a caller that reads only one gets `None` from
    /// the other call and publishes a binding scoped to no version.
    #[serde(alias = "latestActiveImageVersion")]
    pub image_version: Option<String>,
    /// Lifecycle state; an image in CREATING cannot be deleted
    pub state: Option<String>,
}

/// A running MicroVM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Microvm {
    /// MicroVM identifier
    pub microvm_id: Option<String>,
    /// Per-MicroVM HTTPS endpoint the agent protocol travels over
    pub endpoint: Option<String>,
    /// Lifecycle state
    pub state: Option<String>,
    /// Image this MicroVM was started from.
    ///
    /// The only per-session field that says which sandbox owns it — `RunMicrovm` takes no tags,
    /// so without this the answer can only be reached by enumerating the image.
    pub image_arn: Option<String>,
    /// Version of that image.
    pub image_version: Option<String>,
}

/// Response from listing MicroVMs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMicrovmsResponse {
    /// The MicroVMs in scope
    #[serde(default)]
    pub items: Vec<Microvm>,
    /// Continuation token; absent when the last page has been read
    pub next_token: Option<String>,
}

/// Response from listing image versions.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMicrovmImageVersionsResponse {
    /// The versions in scope
    #[serde(default)]
    pub items: Vec<MicrovmImage>,
    /// Continuation token
    pub next_token: Option<String>,
}

/// A token authorising requests to one MicroVM's endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrovmAuthToken {
    /// Header map, not a bearer string. Sending it as `Authorization: Bearer` yields a 403
    /// that reads like a permissions problem rather than a malformed request.
    pub auth_token: std::collections::HashMap<String, String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait LambdaMicrovmsApi: Send + Sync + std::fmt::Debug {
    /// Reads one image.
    async fn get_microvm_image(&self, image_identifier: &str) -> Result<MicrovmImage>;

    /// Deletes an image. Fails while the image is still `CREATING`.
    async fn delete_microvm_image(&self, image_identifier: &str) -> Result<()>;

    /// Lists an image's versions.
    ///
    /// A rolled version stays a cleanup scope until its own MicroVMs are gone, so enumerating
    /// only the newest would orphan every session on the previous one.
    async fn list_microvm_image_versions(
        &self,
        image_identifier: &str,
    ) -> Result<Vec<MicrovmImage>>;

    /// Starts a MicroVM from an image version.
    async fn run_microvm(
        &self,
        image_identifier: &str,
        image_version: &str,
        client_token: &str,
        execution_role_arn: Option<String>,
        network_connectors: Vec<String>,
        idle_suspend_seconds: Option<u32>,
        max_lifetime_seconds: Option<u32>,
    ) -> Result<Microvm>;

    /// Reads one MicroVM.
    async fn get_microvm(&self, microvm_id: &str) -> Result<Microvm>;

    /// Terminates a MicroVM.
    async fn terminate_microvm(&self, microvm_id: &str) -> Result<()>;

    /// Suspends a MicroVM, preserving its filesystem.
    ///
    /// Returns once AWS has accepted the request; the MicroVM reaches `SUSPENDED`
    /// asynchronously, so a caller that needs the state polls `get_microvm`.
    async fn suspend_microvm(&self, microvm_id: &str) -> Result<()>;

    /// Resumes a suspended MicroVM. Also asynchronous.
    async fn resume_microvm(&self, microvm_id: &str) -> Result<()>;

    /// Lists every MicroVM started from one image and version, following pagination.
    ///
    /// Image plus version is the only filter available: `RunMicrovm` takes no `tags`, so a
    /// session carries no label of its own to select on.
    ///
    /// No permission set grants `lambda:ListMicrovms`, because AWS authorizes it against no
    /// resource type and the grant could only be account-wide. A caller wiring this up needs to
    /// add that grant first, or it will compile and then be refused in a customer account.
    async fn list_microvms(
        &self,
        image_identifier: &str,
        image_version: &str,
    ) -> Result<Vec<Microvm>>;

    /// Mints the short-lived token authorising requests to a MicroVM's endpoint.
    ///
    /// `expiration_minutes` is required by the API and capped at 60.
    async fn create_microvm_auth_token(
        &self,
        microvm_id: &str,
        allowed_ports: Vec<u16>,
        expiration_minutes: u32,
    ) -> Result<MicrovmAuthToken>;
}

/// Builds the `RunMicrovm` request body.
///
/// The connector field is `egressNetworkConnectors`, matching
/// `run-microvm --egress-network-connectors`. A MicroVM started without one has public internet
/// access, so getting the name wrong is the opposite of the declared egress policy rather than
/// an error a caller would see.
fn run_microvm_body(
    image_identifier: &str,
    image_version: &str,
    client_token: &str,
    execution_role_arn: Option<String>,
    egress_network_connectors: Vec<String>,
    idle_suspend_seconds: Option<u32>,
    max_lifetime_seconds: Option<u32>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "imageIdentifier": image_identifier,
        "imageVersion": image_version,
        // Idempotency: a retried run must not leave a second MicroVM billing quietly.
        "clientToken": client_token,
    });

    if let Some(role) = execution_role_arn {
        body["executionRoleArn"] = serde_json::Value::String(role);
    }
    if !egress_network_connectors.is_empty() {
        body["egressNetworkConnectors"] = serde_json::json!(egress_network_connectors);
    }
    if let Some(seconds) = idle_suspend_seconds {
        // Suspend only. Auto-resume would bring a session back on a stray request after the
        // caller had moved on, which is a bill and a running sandbox nobody is watching.
        body["idlePolicy"] = serde_json::json!({
            "maxIdleDurationSeconds": seconds,
            "autoResumeEnabled": false,
        });
    }
    // Lambda terminates the MicroVM when this elapses, so it is the ceiling the declaration asked
    // for rather than a hint we would have to police ourselves.
    if let Some(seconds) = max_lifetime_seconds {
        body["maximumDurationInSeconds"] = serde_json::json!(seconds);
    }
    body
}

/// Builds the `CreateMicrovmAuthToken` request body.
///
/// Each port is an object, not a number — `allowedPorts` is a union of port / range / allPorts,
/// and a bare number array is a different request. `expirationInMinutes` is required.
fn auth_token_body(allowed_ports: Vec<u16>, expiration_minutes: u32) -> serde_json::Value {
    let ports: Vec<serde_json::Value> = allowed_ports
        .into_iter()
        .map(|port| serde_json::json!({ "port": port }))
        .collect();

    serde_json::json!({
        "allowedPorts": ports,
        "expirationInMinutes": expiration_minutes.min(MAX_AUTH_TOKEN_MINUTES),
    })
}

/// Client for the Lambda MicroVMs API.
#[derive(Debug, Clone)]
pub struct LambdaMicrovmsClient {
    client: Client,
    credentials: AwsCredentialProvider,
}

impl LambdaMicrovmsClient {
    /// Builds a client from an HTTP client and credentials.
    pub fn new(client: Client, credentials: AwsCredentialProvider) -> Self {
        Self {
            client,
            credentials,
        }
    }

    fn sign_config(&self) -> AwsSignConfig {
        AwsSignConfig {
            service_name: "lambda".into(),
            region: self.credentials.region().to_string(),
            credentials: self.credentials.get_credentials(),
            signing_region: None,
        }
    }

    fn base_url(&self) -> String {
        match self.credentials.get_service_endpoint_option("lambda") {
            Some(override_url) => override_url.to_string(),
            None => format!("https://lambda.{}.amazonaws.com", self.credentials.region()),
        }
    }

    async fn send<T: DeserializeOwned + Send + 'static>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
        operation: &str,
    ) -> Result<T> {
        self.credentials.ensure_fresh().await?;

        let mut url = format!("{}{path}", self.base_url().trim_end_matches('/'));
        if !query.is_empty() {
            let encoded: Vec<String> = query
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{key}={}",
                        form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                    )
                })
                .collect();
            url.push('?');
            url.push_str(&encoded.join("&"));
        }

        let mut builder = self.client.request(method, &url);
        if let Some(body) = body {
            builder = builder.header("content-type", "application/json").body(
                serde_json::to_string(&body).map_err(|error| {
                    AlienError::new(ErrorData::SerializationError {
                        message: format!("Failed to serialize {operation} body: {error}"),
                    })
                })?,
            );
        }

        match sign_send_json(builder, &self.sign_config()).await {
            Ok(value) => Ok(value),
            Err(error) => {
                let data = classify(&error, operation);
                Err(error).context(data)
            }
        }
    }
}

/// Turns a MicroVMs API failure into the error a caller can act on.
///
/// Quota exhaustion has to arrive as `QuotaExceeded` — it is retryable and names the limit — and
/// not as a generic failure, because a deployment that hits the account's MicroVM memory quota
/// should back off rather than surface as a broken sandbox.
fn classify(error: &AlienError<ErrorData>, operation: &str) -> ErrorData {
    // Read structurally and before the rendered-text checks below: a caller has to be able to
    // tell "no such MicroVM" from "the call failed", and `GenericError` carries no status to
    // recover it from. Matching the body text would classify a 500 whose message happens to
    // mention 404 as an absent session.
    if let Some(ErrorData::HttpResponseError { http_status, .. }) = &error.error {
        if *http_status == 404 {
            return ErrorData::RemoteResourceNotFound {
                resource_type: "Microvm".to_string(),
                resource_name: operation.to_string(),
            };
        }
    }

    let rendered = format!("{error:?}");
    if rendered.contains("ServiceQuotaExceededException") {
        ErrorData::QuotaExceeded {
            message: format!(
                "Lambda MicroVMs {operation}: account quota exhausted. MicroVM memory is limited \
                 per account and Region; retry after existing sandboxes terminate, or request an \
                 increase."
            ),
        }
    } else if rendered.contains("ThrottlingException")
        || rendered.contains("TooManyRequestsException")
    {
        ErrorData::RateLimitExceeded {
            message: format!("Lambda MicroVMs {operation} was throttled"),
        }
    } else {
        ErrorData::GenericError {
            message: format!("Lambda MicroVMs {operation} failed"),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl LambdaMicrovmsApi for LambdaMicrovmsClient {
    async fn get_microvm_image(&self, image_identifier: &str) -> Result<MicrovmImage> {
        self.send(
            Method::GET,
            &format!("/{API_VERSION}/microvm-images/{image_identifier}"),
            &[],
            None,
            "GetMicrovmImage",
        )
        .await
    }

    async fn delete_microvm_image(&self, image_identifier: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::DELETE,
                &format!("/{API_VERSION}/microvm-images/{image_identifier}"),
                &[],
                None,
                "DeleteMicrovmImage",
            )
            .await?;
        Ok(())
    }

    async fn list_microvm_image_versions(
        &self,
        image_identifier: &str,
    ) -> Result<Vec<MicrovmImage>> {
        let mut versions = Vec::new();
        let mut next: Option<String> = None;

        loop {
            let query: Vec<(&str, String)> = next
                .as_ref()
                .map(|token| vec![("nextToken", token.clone())])
                .unwrap_or_default();

            let page: ListMicrovmImageVersionsResponse = self
                .send(
                    Method::GET,
                    &format!("/{API_VERSION}/microvm-images/{image_identifier}/versions"),
                    &query,
                    None,
                    "ListMicrovmImageVersions",
                )
                .await?;

            versions.extend(page.items);
            next = page.next_token;
            if next.is_none() {
                return Ok(versions);
            }
        }
    }

    async fn run_microvm(
        &self,
        image_identifier: &str,
        image_version: &str,
        client_token: &str,
        execution_role_arn: Option<String>,
        network_connectors: Vec<String>,
        idle_suspend_seconds: Option<u32>,
        max_lifetime_seconds: Option<u32>,
    ) -> Result<Microvm> {
        let body = run_microvm_body(
            image_identifier,
            image_version,
            client_token,
            execution_role_arn,
            network_connectors,
            idle_suspend_seconds,
            max_lifetime_seconds,
        );

        self.send(
            Method::POST,
            &format!("/{API_VERSION}/microvms"),
            &[],
            Some(body),
            "RunMicrovm",
        )
        .await
    }

    async fn get_microvm(&self, microvm_id: &str) -> Result<Microvm> {
        self.send(
            Method::GET,
            &format!("/{API_VERSION}/microvms/{microvm_id}"),
            &[],
            None,
            "GetMicrovm",
        )
        .await
    }

    async fn terminate_microvm(&self, microvm_id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::DELETE,
                &format!("/{API_VERSION}/microvms/{microvm_id}"),
                &[],
                None,
                "TerminateMicrovm",
            )
            .await?;
        Ok(())
    }

    async fn suspend_microvm(&self, microvm_id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::POST,
                &format!("/{API_VERSION}/microvms/{microvm_id}/suspend"),
                &[],
                Some(serde_json::json!({})),
                "SuspendMicrovm",
            )
            .await?;
        Ok(())
    }

    async fn resume_microvm(&self, microvm_id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                Method::POST,
                &format!("/{API_VERSION}/microvms/{microvm_id}/resume"),
                &[],
                Some(serde_json::json!({})),
                "ResumeMicrovm",
            )
            .await?;
        Ok(())
    }

    async fn list_microvms(
        &self,
        image_identifier: &str,
        image_version: &str,
    ) -> Result<Vec<Microvm>> {
        let mut microvms = Vec::new();
        let mut next: Option<String> = None;

        loop {
            let mut query = vec![
                ("imageIdentifier", image_identifier.to_string()),
                ("imageVersion", image_version.to_string()),
            ];
            if let Some(token) = next.as_ref() {
                query.push(("nextToken", token.clone()));
            }

            let page: ListMicrovmsResponse = self
                .send(
                    Method::GET,
                    &format!("/{API_VERSION}/microvms"),
                    &query,
                    None,
                    "ListMicrovms",
                )
                .await?;

            microvms.extend(page.items);
            next = page.next_token;
            // Paginate to exhaustion: a partial list during teardown silently orphans whatever
            // sat on the pages nobody read.
            if next.is_none() {
                return Ok(microvms);
            }
        }
    }

    async fn create_microvm_auth_token(
        &self,
        microvm_id: &str,
        allowed_ports: Vec<u16>,
        expiration_minutes: u32,
    ) -> Result<MicrovmAuthToken> {
        self.send(
            Method::POST,
            &format!("/{API_VERSION}/microvms/{microvm_id}/auth-token"),
            &[],
            Some(auth_token_body(allowed_ports, expiration_minutes)),
            "CreateMicrovmAuthToken",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declared ceiling has to reach the wire: Lambda is what terminates the MicroVM when it
    /// elapses, so a value we accept and drop would leave a sandbox running past a limit its
    /// stack declared — the shape of claim this resource exists to keep.
    #[test]
    fn a_declared_lifetime_reaches_the_run_request() {
        let body = run_microvm_body("img", "1", "token", None, vec![], None, Some(3600));
        assert_eq!(body["maximumDurationInSeconds"], serde_json::json!(3600));

        let unbounded = run_microvm_body("img", "1", "token", None, vec![], None, None);
        assert!(
            unbounded.get("maximumDurationInSeconds").is_none(),
            "an undeclared ceiling stays absent rather than becoming a made-up one"
        );
    }

    /// Built the way the transport builds it, not by hand: `handle_json_response` raises
    /// `HttpResponseError`, whose variant declares no status of its own, so anything reading
    /// `http_status_code` sees the derive's 500 default and never sees the 404. A caller has to be
    /// able to tell an absent MicroVM from a failed call, so the status is read structurally here
    /// and re-raised as the variant that carries it.
    #[test]
    fn an_absent_microvm_is_classified_as_not_found() {
        let response = AlienError::new(ErrorData::HttpResponseError {
            message: "GetMicrovm failed".to_string(),
            url: "https://lambda.example.invalid/microvms/mv-1".to_string(),
            http_status: 404,
            http_request_text: None,
            http_response_text: Some("{\"message\":\"Microvm not found\"}".to_string()),
        });

        assert!(
            matches!(
                classify(&response, "GetMicrovm"),
                ErrorData::RemoteResourceNotFound { .. }
            ),
            "a 404 has to survive as something the caller can match on"
        );

        // The trap this replaced: the status never reaches the wrapped error.
        assert_ne!(
            response.http_status_code,
            Some(404),
            "if this ever becomes Some(404), the status is readable directly and the arm above \
             can be simplified"
        );
    }

    /// A body that merely mentions 404 is not an absent MicroVM. Classifying on rendered text
    /// would turn a throttle or a server error into "the session is gone", which starts a second
    /// sandbox while the first keeps running.
    #[test]
    fn only_the_status_makes_a_microvm_absent() {
        for status in [429, 500, 503] {
            let error = AlienError::new(ErrorData::HttpResponseError {
                message: "GetMicrovm failed".to_string(),
                url: "https://lambda.example.invalid/microvms/404".to_string(),
                http_status: status,
                http_request_text: None,
                http_response_text: Some("upstream said 404 somewhere".to_string()),
            });

            assert!(
                !matches!(
                    classify(&error, "GetMicrovm"),
                    ErrorData::RemoteResourceNotFound { .. }
                ),
                "{status} is not an absent MicroVM"
            );
        }
    }

    /// Paths were read out of the published JS SDK, not guessed. Pinning them here
    /// means a future edit that mistypes one fails at build time rather than as a 404 that
    /// reads like a permissions error.
    #[test]
    fn the_api_version_matches_the_published_wire_contract() {
        assert_eq!(API_VERSION, "2025-09-09");
    }

    /// The egress connector is the whole of what makes `egress: deny` real, and the failure
    /// mode of naming its field wrong is a MicroVM with public internet access rather than a
    /// rejected request. AWS spells it `--egress-network-connectors` on `run-microvm`.
    #[test]
    fn a_session_carries_its_egress_connector_under_the_name_aws_reads() {
        let body = run_microvm_body(
            "image",
            "3",
            "token",
            None,
            vec!["arn:aws:lambda:us-west-2:123456789012:network-connector:sbx".to_string()],
            None,
            None,
        );

        assert_eq!(
            body["egressNetworkConnectors"],
            serde_json::json!(["arn:aws:lambda:us-west-2:123456789012:network-connector:sbx"])
        );
        assert!(
            body.get("networkConnectors").is_none(),
            "the connector must not also ride a field RunMicrovm does not read: {body}"
        );
    }

    /// Both fields were wrong in the first cut: ports encoded as bare numbers, and
    /// `expirationInMinutes` omitted entirely though the API requires it. Neither would have
    /// failed until a live call, so the shape that works is pinned here.
    #[test]
    fn the_auth_token_request_encodes_ports_as_objects() {
        let body = auth_token_body(vec![8971], 30);

        assert_eq!(
            body["allowedPorts"],
            serde_json::json!([{ "port": 8971 }]),
            "allowedPorts is a union of port/range/allPorts, not a list of numbers"
        );
        assert_eq!(body["expirationInMinutes"], 30);
    }

    /// AWS caps the token at 60 minutes; asking for more is a rejected request, so the clamp
    /// happens here rather than arriving as a validation error at the call site.
    #[test]
    fn an_over_long_expiry_is_clamped_to_the_documented_maximum() {
        assert_eq!(auth_token_body(vec![80], 600)["expirationInMinutes"], 60);
    }

    /// Quota exhaustion must arrive as a typed, retryable error naming the
    /// quota. It is the one failure a caller should back off from rather than treat as a broken
    /// sandbox, and a generic error gives them nothing to branch on.
    #[test]
    fn quota_exhaustion_is_typed_and_retryable() {
        let raw = AlienError::new(ErrorData::GenericError {
            message: "ServiceQuotaExceededException: memory limit".to_string(),
        });

        let data = classify(&raw, "RunMicrovm");
        let error = AlienError::new(data);

        assert_eq!(error.code, "QUOTA_EXCEEDED");
        assert!(error.retryable, "a quota clears when sandboxes terminate");
        assert!(
            error.to_string().contains("quota exhausted"),
            "the message must name the condition: {error}"
        );
    }

    #[test]
    fn throttling_is_distinguished_from_quota() {
        let raw = AlienError::new(ErrorData::GenericError {
            message: "ThrottlingException: slow down".to_string(),
        });

        assert_eq!(
            AlienError::new(classify(&raw, "RunMicrovm")).code,
            "RATE_LIMIT_EXCEEDED"
        );
    }

    /// Anything unrecognised stays generic rather than being guessed into a typed error a
    /// caller would then branch on incorrectly.
    #[test]
    fn an_unrecognised_failure_is_not_invented_into_a_typed_error() {
        let raw = AlienError::new(ErrorData::GenericError {
            message: "ValidationException: bad ARN".to_string(),
        });

        assert_eq!(
            AlienError::new(classify(&raw, "RunMicrovm")).code,
            "GENERIC_ERROR"
        );
    }

    #[test]
    fn an_auth_token_is_a_header_map_not_a_bearer_string() {
        let token: MicrovmAuthToken = serde_json::from_str(
            r#"{"authToken":{"X-aws-proxy-auth":"eyJ...","X-aws-proxy-port":"8080"}}"#,
        )
        .expect("deserializes");

        assert_eq!(token.auth_token.len(), 2);
        assert!(token.auth_token.contains_key("X-aws-proxy-auth"));
    }

    /// Reading a response key that does not exist reports a working call as a failure.
    /// Listing keys off `items` is the shape the API actually returns.
    #[test]
    fn a_list_response_reads_items_and_a_next_token() {
        let page: ListMicrovmsResponse = serde_json::from_str(
            r#"{"items":[{"microvmId":"microvm-1","endpoint":"abc.lambda-url","state":"RUNNING"}],"nextToken":"t2"}"#,
        )
        .expect("deserializes");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].microvm_id.as_deref(), Some("microvm-1"));
        assert_eq!(page.next_token.as_deref(), Some("t2"));
    }

    #[test]
    fn an_empty_list_response_is_not_an_error() {
        let page: ListMicrovmsResponse =
            serde_json::from_str("{}").expect("an absent items key means none, not a parse error");
        assert!(page.items.is_empty());
        assert!(page.next_token.is_none());
    }
}

/// Calls `CreateMicrovmImage` directly, so the service's own error is visible.
///
/// CloudControl reports a failed image build as `NotStabilized` with no `StateReason` and no log
/// group, which says nothing about why. This goes straight at the API. Lives here rather than in
/// `tests/` because `send` is private, and adding a production `create_microvm_image` for a
/// diagnostic would be a method nothing else calls — setup builds images through IaC.
#[cfg(test)]
mod live_image_create {
    use super::*;
    use crate::aws::aws_request_utils::AwsRequestSigner;
    use crate::{AwsCredentialProvider, AwsCredentials};
    use std::path::PathBuf as StdPathBuf;

    fn client() -> LambdaMicrovmsClient {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).ok();

        let config = crate::AwsClientConfig {
            account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").expect("AWS_TARGET_ACCOUNT_ID"),
            region: std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION"),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID")
                    .expect("AWS_TARGET_ACCESS_KEY_ID"),
                secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY")
                    .expect("AWS_TARGET_SECRET_ACCESS_KEY"),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN")
                    .ok()
                    .filter(|token| !token.is_empty()),
            },
            service_overrides: None,
        };

        LambdaMicrovmsClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        )
    }

    /// Deletes an image the way the API wants it, versions first.
    ///
    /// CloudControl accepts a delete on a `CREATED` image and never removes it — the versions
    /// hold it. `DeleteMicrovmImageVersion` is in `sandbox/provision` for exactly this reason.
    #[tokio::test]
    #[ignore]
    async fn delete_images_versions_first() {
        let client = client();
        for name in std::env::var("PROBE_IMAGES")
            .expect("PROBE_IMAGES")
            .split(',')
        {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let versions = client.list_microvm_image_versions(name).await;
            match &versions {
                Ok(list) => println!("{name}: {} version(s)", list.len()),
                Err(error) => println!("{name}: list refused: {error}"),
            }
            for version in versions.unwrap_or_default() {
                let Some(v) = version.image_version.as_deref() else {
                    continue;
                };
                let path = format!("/{API_VERSION}/microvm-images/{name}/versions/{v}");
                let result: std::result::Result<serde_json::Value, _> = client
                    .send(
                        Method::DELETE,
                        &path,
                        &[],
                        None,
                        "DeleteMicrovmImageVersion",
                    )
                    .await;
                println!(
                    "  version {v}: {}",
                    if result.is_ok() { "deleted" } else { "refused" }
                );
            }
            match client.delete_microvm_image(name).await {
                Ok(()) => println!("  image deleted"),
                Err(error) => println!("  image refused: {error}"),
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn create_an_image_and_report_the_services_own_error() {
        let name = std::env::var("PROBE_IMAGE_NAME").expect("PROBE_IMAGE_NAME");
        let build_role = std::env::var("PROBE_BUILD_ROLE_ARN").expect("PROBE_BUILD_ROLE_ARN");
        let artifact = std::env::var("PROBE_ARTIFACT_URI").expect("PROBE_ARTIFACT_URI");
        let region = std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION");

        let mut body = serde_json::json!({
            "name": name,
            "baseImageArn": format!("arn:aws:lambda:{region}:aws:microvm-image:al2023-1"),
            "buildRoleArn": build_role,
            "codeArtifact": { "uri": artifact },
        });
        for extra in std::env::var("PROBE_EXTRA").unwrap_or_default().split(',') {
            match extra.trim() {
                "hooks" => {
                    body["hooks"] = serde_json::json!({
                        "port": 8971,
                        "microvmImageHooks": { "ready": "ENABLED", "readyTimeoutInSeconds": 120 },
                        "microvmHooks": { "run": "ENABLED", "runTimeoutInSeconds": 30,
                                          "resume": "ENABLED", "resumeTimeoutInSeconds": 30 }
                    })
                }
                "env" => {
                    body["environmentVariables"] = serde_json::json!({
                        "ALIEN_SANDBOX_ROOT": "/sandbox",
                        "ALIEN_SANDBOX_PORT": "8971",
                        "ALIEN_SANDBOX_AUTHORIZATION": "transport",
                        "ALIEN_SANDBOX_EXEC_UID": "60000",
                        "ALIEN_SANDBOX_EXEC_GID": "60000"
                    })
                }
                "cpu" => {
                    body["cpuConfigurations"] = serde_json::json!([{ "architecture": "ARM_64" }])
                }
                "res" => body["resources"] = serde_json::json!([{ "minimumMemoryInMiB": 512 }]),
                "log" => {
                    body["logging"] = serde_json::json!({ "cloudWatch": { "logGroup": std::env::var("PROBE_LOG_GROUP").expect("PROBE_LOG_GROUP") } })
                }
                "conn" => {
                    if let Some(c) = std::env::var("PROBE_CONNECTOR_ARN").ok() {
                        body["egressNetworkConnectors"] = serde_json::json!([c]);
                    }
                }
                _ => {}
            }
        }
        println!("request: {}", serde_json::to_string_pretty(&body).unwrap());

        // Signed by hand rather than through `send`, which drops the response body — and the
        // body is where AWS puts the reason a 400 happened.
        let client = client();
        client
            .credentials
            .ensure_fresh()
            .await
            .expect("credentials");
        let url = format!("{}/{API_VERSION}/microvm-images", client.base_url());
        let signed = client
            .client
            .request(Method::POST, &url)
            .header("content-type", "application/json")
            .body(serde_json::to_string(&body).unwrap())
            .sign_aws_request(&client.sign_config())
            .expect("signing");

        let response = signed.send().await.expect("the request should reach AWS");
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        println!("STATUS: {status}");
        println!("BODY: {text}");
    }

    /// Runs the real `run_microvm_body` against a live image and prints what AWS answered.
    ///
    /// A workload only ever sees `SANDBOX_UNREACHABLE`, because the cause does not survive the
    /// hop into the SDK. This asks the same question with the reply left intact.
    ///
    /// Starts a billable MicroVM that nothing else reclaims — `terminate_probe_microvms` is its
    /// pair.
    #[tokio::test]
    #[ignore]
    async fn run_a_microvm_and_report_the_services_own_error() {
        let body = run_microvm_body(
            &std::env::var("PROBE_IMAGE_ARN").expect("PROBE_IMAGE_ARN"),
            &std::env::var("PROBE_IMAGE_VERSION").unwrap_or_else(|_| "1.0".to_string()),
            "probe-client-token",
            std::env::var("PROBE_EXEC_ROLE_ARN").ok(),
            std::env::var("PROBE_CONNECTOR_ARN")
                .ok()
                .into_iter()
                .collect(),
            None,
            None,
        );
        println!("request: {}", serde_json::to_string_pretty(&body).unwrap());

        let client = client();
        client
            .credentials
            .ensure_fresh()
            .await
            .expect("credentials");
        let url = format!("{}/{API_VERSION}/microvms", client.base_url());
        let signed = client
            .client
            .request(Method::POST, &url)
            .header("content-type", "application/json")
            .body(serde_json::to_string(&body).unwrap())
            .sign_aws_request(&client.sign_config())
            .expect("signing");

        let response = signed.send().await.expect("the request should reach AWS");
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        println!("STATUS: {status}");
        println!("BODY: {text}");
    }

    /// Terminates the MicroVMs a probe run started.
    ///
    /// A probe MicroVM outlives the stack it was launched from and bills for up to eight hours,
    /// and no teardown owns it — the image delete does not reach it.
    #[tokio::test]
    #[ignore]
    async fn terminate_probe_microvms() {
        let client = client();
        for id in std::env::var("PROBE_MICROVM_IDS")
            .expect("PROBE_MICROVM_IDS")
            .split(',')
        {
            let id = id.trim();
            if id.is_empty() {
                continue;
            }
            match client.terminate_microvm(id).await {
                Ok(()) => println!("{id}: terminated"),
                Err(error) => println!("{id}: refused: {error}"),
            }
        }
    }
}

/// A MicroVM under `deny` cannot reach the internet.
///
/// Two MicroVMs from one image — one started with the egress connector the emitted packages
/// render, one
/// with none — each asked to reach a public address through the agent's own exec path. The
/// comparison is the point: a `deny` MicroVM that cannot reach the internet proves nothing if the
/// control cannot either.
#[cfg(test)]
mod live_deny {
    use super::*;
    use crate::{AwsCredentialProvider, AwsCredentials};
    use std::path::PathBuf as StdPathBuf;
    use std::time::Duration;

    fn client() -> LambdaMicrovmsClient {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).ok();
        let config = crate::AwsClientConfig {
            account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").expect("AWS_TARGET_ACCOUNT_ID"),
            region: std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION"),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID").expect("key"),
                secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY").expect("secret"),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN")
                    .ok()
                    .filter(|token| !token.is_empty()),
            },
            service_overrides: None,
        };
        LambdaMicrovmsClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        )
    }

    /// Runs one command through the agent and returns its raw NDJSON body.
    async fn exec(
        client: &LambdaMicrovmsClient,
        id: &str,
        endpoint: &str,
        command: &[&str],
    ) -> String {
        let token = client
            .create_microvm_auth_token(id, vec![8971], 10)
            .await
            .expect("CreateMicrovmAuthToken");
        let mut request = Client::new()
            .post(format!("https://{endpoint}/v1/exec"))
            .header("X-aws-proxy-port", "8971")
            .json(&serde_json::json!({"command": command, "deadlineMs": 15000}));
        for (name, value) in token.auth_token {
            request = request.header(name, value);
        }
        let body = match request.send().await {
            Ok(response) => response.text().await.unwrap_or_default(),
            Err(error) => return format!("<transport error: {error}>"),
        };

        // The agent streams NDJSON frames whose `data` is base64, so a plain string match against
        // the body would test the encoding rather than what the command printed.
        use base64::Engine as _;
        let mut decoded = String::new();
        for line in body.lines() {
            let Ok(frame) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(data) = frame.get("data").and_then(|d| d.as_str()) {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                    decoded.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
            if let Some(code) = frame.get("code") {
                decoded.push_str(&format!("\nexit={code}"));
            }
        }
        decoded
    }

    async fn start(
        client: &LambdaMicrovmsClient,
        image: &str,
        version: &str,
        connectors: Vec<String>,
    ) -> (String, String) {
        let token = uuid::Uuid::new_v4().simple().to_string();
        // The execution role is what a credential probe reaches for, so a session started
        // without one cannot answer whether it is reachable.
        let role = std::env::var("PROBE_EXEC_ROLE_ARN")
            .ok()
            .filter(|arn| !arn.is_empty());
        let microvm = client
            .run_microvm(image, version, &token, role, connectors, None, None)
            .await
            .expect("RunMicrovm");
        let id = microvm.microvm_id.expect("a MicroVM id");
        for _ in 0..60 {
            let current = client.get_microvm(&id).await.expect("get_microvm");
            if current.state.as_deref() == Some("RUNNING") {
                return (id, current.endpoint.expect("an endpoint"));
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        panic!("MicroVM {id} never reached RUNNING");
    }

    /// Which form of `imageIdentifier` each call accepts.
    ///
    /// `RunMicrovm` refuses a bare name with "Malformed ARN - doesn't start with 'arn:'", and the
    /// import data feeds both this and `GetMicrovmImage`, so the two have to agree.
    #[tokio::test]
    #[ignore]
    async fn which_image_identifier_form_each_call_accepts() {
        let name = std::env::var("PROBE_IMAGE_NAME").expect("PROBE_IMAGE_NAME");
        let arn = std::env::var("PROBE_IMAGE_ARN").expect("PROBE_IMAGE_ARN");
        let client = client();

        for (label, identifier) in [("name", &name), ("arn", &arn)] {
            let got = client.get_microvm_image(identifier).await;
            println!(
                "GetMicrovmImage({label}) -> {}",
                if got.is_ok() { "ok" } else { "REFUSED" }
            );

            let token = uuid::Uuid::new_v4().simple().to_string();
            let ran = client
                .run_microvm(identifier, "1.0", &token, None, Vec::new(), None, None)
                .await;
            match ran {
                Ok(microvm) => {
                    println!("RunMicrovm({label}) -> ok");
                    if let Some(id) = microvm.microvm_id {
                        let _ = client.terminate_microvm(&id).await;
                    }
                }
                Err(error) => println!(
                    "RunMicrovm({label}) -> REFUSED: {}",
                    format!("{error:?}")
                        .split("http_response_text")
                        .nth(1)
                        .unwrap_or("")
                        .chars()
                        .take(90)
                        .collect::<String>()
                ),
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn a_denied_sandbox_cannot_reach_the_internet_and_an_open_one_can() {
        let image = std::env::var("PROBE_IMAGE_NAME").expect("PROBE_IMAGE_NAME");
        let version = std::env::var("PROBE_IMAGE_VERSION").unwrap_or_else(|_| "1.0".to_string());
        let connector = std::env::var("PROBE_CONNECTOR_ARN").expect("PROBE_CONNECTOR_ARN");
        let client = client();

        let probe = [
            "/usr/bin/curl",
            "-sS",
            "--max-time",
            "8",
            "-o",
            "/dev/null",
            "-w",
            "HTTP:%{http_code}",
            "https://example.com",
        ];

        let (denied, denied_endpoint) = start(&client, &image, &version, vec![connector]).await;
        println!("DENY microvm={denied}");
        let denied_output = exec(&client, &denied, &denied_endpoint, &probe).await;
        println!("DENY OUTPUT: {denied_output}");
        let _ = client.terminate_microvm(&denied).await;

        let (open, open_endpoint) = start(&client, &image, &version, Vec::new()).await;
        println!("CONTROL microvm={open}");
        let open_output = exec(&client, &open, &open_endpoint, &probe).await;
        println!("CONTROL OUTPUT: {open_output}");
        let _ = client.terminate_microvm(&open).await;

        // The control first. A `deny` MicroVM that cannot reach the internet proves nothing if
        // the image, the agent or the probe was broken for both.
        assert!(
            open_output.contains("HTTP:200"),
            "the control must reach the internet, or the deny result means nothing:\n{open_output}"
        );
        assert!(
            !denied_output.contains("HTTP:200"),
            "a sandbox under deny reached the internet:\n{denied_output}"
        );
        assert!(
            denied_output.contains("HTTP:000"),
            "deny should fail to connect rather than get some other status:\n{denied_output}"
        );
    }

    /// Records what AWS does today: an egress connector governs routed traffic and not metadata.
    ///
    /// This characterizes the platform, it does not guard a boundary — it asserts the execution
    /// role IS readable, so it fails if AWS ever stops serving the IAM tree. That would be good
    /// news, and the fix is to delete this test rather than to restore the behaviour. The test
    /// that guards something is `an_open_sandbox_reaches_the_internet_but_no_credentials`, and
    /// the control is `a_sandbox_binding_naming_an_execution_role_is_refused` in alien-bindings,
    /// which is what stops a role reaching a session at all.
    ///
    /// Needs a built image, an execution role and a deny connector:
    ///
    /// ```text
    /// PROBE_IMAGE_NAME=arn:aws:lambda:<region>:<account>:microvm-image:<name>  # ARN, not a name
    /// PROBE_IMAGE_VERSION=1.0
    /// PROBE_EXEC_ROLE_ARN=arn:aws:iam::<account>:role/<role>
    /// PROBE_CONNECTOR_ARN=arn:aws:lambda:<region>:<account>:network-connector:<id>
    /// AWS_TARGET_{ACCOUNT_ID,REGION,ACCESS_KEY_ID,SECRET_ACCESS_KEY,SESSION_TOKEN}
    /// ```
    ///
    /// Prints identifiers only, never the secret — the question is reachability.
    #[tokio::test]
    #[ignore]
    async fn an_egress_connector_governs_routed_traffic_and_not_metadata() {
        let image = std::env::var("PROBE_IMAGE_NAME").expect("PROBE_IMAGE_NAME");
        let version = std::env::var("PROBE_IMAGE_VERSION").unwrap_or_else(|_| "1.0".to_string());
        let connector = std::env::var("PROBE_CONNECTOR_ARN").expect("PROBE_CONNECTOR_ARN");
        assert!(
            std::env::var("PROBE_EXEC_ROLE_ARN").is_ok_and(|arn| !arn.is_empty()),
            "this proves what a session can do with a role attached, so one must be set"
        );
        let client = client();

        let script = "\
code() { curl -sS --max-time 5 -o /dev/null -w '%{http_code}' \"$@\" || true; }; \
echo \"INTERNET=$(code https://example.com)\"; \
T=$(curl -sS --max-time 5 -X PUT -H 'X-aws-ec2-metadata-token-ttl-seconds: 60' \
    http://169.254.169.254/latest/api/token || true); \
R=$(curl -sS --max-time 5 -H \"X-aws-ec2-metadata-token: $T\" \
    http://169.254.169.254/latest/meta-data/iam/security-credentials/ || true); \
echo \"ROLE=$R\"; \
C=$(curl -sS --max-time 5 -H \"X-aws-ec2-metadata-token: $T\" \
    \"http://169.254.169.254/latest/meta-data/iam/security-credentials/$R\" || true); \
echo \"HAS_KEY=$(echo \"$C\" | grep -c AccessKeyId)\"; \
echo \"HAS_SECRET=$(echo \"$C\" | grep -c SecretAccessKey)\"; \
echo \"CODE=$(echo \"$C\" | sed -n 's/.*\"Code\" *: *\"\\([A-Za-z]*\\)\".*/\\1/p')\"; \
echo \"EXPIRES=$(echo \"$C\" | sed -n 's/.*\"Expiration\" *: *\"\\([^\"]*\\)\".*/\\1/p')\"";

        let mut seen = Vec::new();
        for (mode, connectors) in [("DENY ", vec![connector.clone()]), ("ALLOW", Vec::new())] {
            let (id, endpoint) = start(&client, &image, &version, connectors).await;
            let out = exec(&client, &id, &endpoint, &["/bin/sh", "-c", script]).await;
            let _ = client.terminate_microvm(&id).await;
            let field = |name: &str| {
                out.lines()
                    .find_map(|line| line.strip_prefix(&format!("{name}=")))
                    .unwrap_or_else(|| panic!("{mode} probe did not report {name}:\n{out}"))
                    .trim()
                    .to_string()
            };
            let (internet, role, key) = (field("INTERNET"), field("ROLE"), field("HAS_KEY"));
            println!(
                "{mode} internet={internet} role={role} has_key={key} code={} expires={}",
                field("CODE"),
                field("EXPIRES")
            );
            seen.push((mode, internet, role, key));
        }

        let deny = &seen[0];
        let allow = &seen[1];

        // The connector does what it claims for routed traffic; without this the comparison
        // below would be measuring a broken image.
        assert_ne!(
            deny.1, "200",
            "a deny session reached the internet: {deny:?}"
        );
        assert_eq!(
            allow.1, "200",
            "an allow session could not reach the internet: {allow:?}"
        );

        // And the point: the same role is readable in both, so the connector never governed it.
        for (mode, _, role, key) in &seen {
            assert!(
                !role.is_empty() && key == "1",
                "{mode} could not read the execution role, which would mean metadata access \
                 depends on the connector after all: role={role:?} has_key={key:?}"
            );
        }
    }

    /// What an `allow` sandbox can reach besides the internet.
    ///
    /// `allow` is a MicroVM started with no egress connector, which leaves AWS's managed internet
    /// path in place. Private ranges and the deployment's VPC fall away by construction — neither
    /// is routable from outside the customer VPC — but link-local never traversed a connector, so
    /// nothing about removing one speaks to it. The metadata service does answer a session, and
    /// serves `placement/` and `tags/` only: the credential tree is absent, which is the whole
    /// reason `allow` is safe for untrusted code. That is a property of the MicroVM runtime, not
    /// of the declaration, so it is asserted here rather than assumed.
    #[tokio::test]
    #[ignore]
    async fn an_open_sandbox_reaches_the_internet_but_no_credentials() {
        let image = std::env::var("PROBE_IMAGE_NAME").expect("PROBE_IMAGE_NAME");
        let version = std::env::var("PROBE_IMAGE_VERSION").unwrap_or_else(|_| "1.0".to_string());
        let client = client();

        let (id, endpoint) = start(&client, &image, &version, Vec::new()).await;
        println!("ALLOW microvm={id}");

        // One session, one script: six round trips through the agent would each pay the token
        // and connection cost, and a partial failure would leave a session running.
        let script = "\
code() { curl -sS --max-time 5 -o /dev/null -w '%{http_code}' \"$@\" || true; }; \
T=$(curl -sS --max-time 5 -X PUT -H 'X-aws-ec2-metadata-token-ttl-seconds: 60' \
    http://169.254.169.254/latest/api/token || true); \
echo \"INTERNET=$(code https://example.com)\"; \
echo \"IMDS_ROOT=$(curl -sS --max-time 5 -H \"X-aws-ec2-metadata-token: $T\" \
    http://169.254.169.254/latest/meta-data/ | tr '\\n' ' ')\"; \
echo \"IMDS_CREDS=$(code -H \"X-aws-ec2-metadata-token: $T\" \
    http://169.254.169.254/latest/meta-data/iam/security-credentials/)\"; \
echo \"ECS_CREDS=$(code http://169.254.170.2/v2/credentials/)\"; \
echo \"RFC1918=$(code http://10.0.0.1/)\"; \
echo \"AWSENV=$(env | grep -c -i '^AWS_\\|SECRET\\|SESSION_TOKEN' || true)\"";

        let out = exec(&client, &id, &endpoint, &["/bin/sh", "-c", script]).await;
        let _ = client.terminate_microvm(&id).await;
        println!("{out}");

        let field = |name: &str| {
            out.lines()
                .find_map(|line| line.strip_prefix(&format!("{name}=")))
                .unwrap_or_else(|| panic!("probe did not report {name}:\n{out}"))
                .trim()
                .to_string()
        };

        // The control first: a session that reached nothing would satisfy every check below while
        // proving only that the probe was broken.
        assert_eq!(
            field("INTERNET"),
            "200",
            "an allow sandbox must reach the internet, or the rest of this proves nothing:\n{out}"
        );

        // The token endpoint answering is not the risk and is not asserted against — what would
        // make `allow` a credential path is the IAM tree behind it existing.
        let root = field("IMDS_ROOT");
        assert!(
            !root.contains("iam"),
            "the metadata service now offers an iam tree to a session, so an allow sandbox can \
             reach the execution role: {root}"
        );
        assert_ne!(
            field("IMDS_CREDS"),
            "200",
            "instance metadata served credentials to an allow sandbox:\n{out}"
        );
        assert_ne!(
            field("ECS_CREDS"),
            "200",
            "the container credential endpoint answered an allow sandbox:\n{out}"
        );
        assert_ne!(
            field("RFC1918"),
            "200",
            "an allow sandbox reached a private address:\n{out}"
        );
        assert_eq!(
            field("AWSENV"),
            "0",
            "the exec user can see AWS credential variables:\n{out}"
        );
    }
}
