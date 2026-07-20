//! Credential resolution and minting endpoints.

use alien_azure_clients::AzureClientConfigExt;
use alien_bindings::traits::ImpersonationRequest;
use alien_bindings::ServiceAccountInfo;
use alien_core::{
    AwsCredentials, AzureCredentials, ClientConfig, Container, Daemon, GcpCredentials, Platform,
    ServiceAccount, Worker,
};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::GcpClientConfigExt;
use axum::{
    extract::{Json, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

use crate::auth::Subject;
use crate::error::ErrorData;
use crate::ids::sha256_hash;
use crate::traits::DeploymentRecord;

use super::{auth, AppState};

// --- Mint constants ---

/// Minimum session duration the mint endpoint will grant, in seconds.
const MIN_DURATION_SECONDS: i32 = 900;
/// Maximum session duration the mint endpoint will grant, in seconds.
const MAX_DURATION_SECONDS: i32 = 3600;
/// Default session duration when the caller omits `durationSeconds`.
const DEFAULT_DURATION_SECONDS: i32 = 3600;
/// Maximum length of an STS `RoleSessionName`. Session names longer than this
/// are hash-suffix truncated (see [`mint_session_name`]).
const MAX_SESSION_NAME_LEN: usize = 64;
/// GCP access tokens minted by this endpoint use the broad cloud-platform
/// scope; the service account's IAM grants remain the authorization boundary.
const GCP_CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
/// The exact OAuth scopes used by Alien's Azure bindings. Azure access tokens
/// are audience-specific, so one management token cannot safely stand in for
/// storage, Key Vault, or Service Bus.
const AZURE_MINT_SCOPES: [&str; 4] = [
    "https://management.azure.com/.default",
    "https://storage.azure.com/.default",
    "https://vault.azure.net/.default",
    "https://servicebus.azure.net/.default",
];

// --- Request / Response types ---

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsRequest {
    pub deployment_id: String,
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResolveCredentialsResponse {
    pub client_config: ClientConfig,
}

/// Manual `Debug`: `ClientConfig` carries live credentials. Never let a
/// `{:?}` of this response (log line, panic message, test failure output)
/// print them — even indirectly through `serde_json::Value`, which has no
/// redaction of its own once the typed config is serialized.
impl std::fmt::Debug for ResolveCredentialsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveCredentialsResponse")
            .field("client_config", &"<redacted>")
            .finish()
    }
}

/// Request body for `POST /v1/credentials/mint`.
///
/// `deny_unknown_fields` so clients cannot smuggle in resolver internals
/// (platform, stack state, etc.) — the server derives everything from the
/// authenticated deployment.
#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MintCredentialsRequest {
    /// Deployment to mint credentials for. The caller's bearer token must be
    /// this deployment's token (or a workspace-admin token).
    pub deployment_id: String,
    /// Current-release compute resource requesting the credentials. The
    /// resource must depend on `bindingName` as a service-account resource.
    pub resource_id: String,
    /// Service-account binding to impersonate on the target platform.
    pub binding_name: String,
    /// Requested lifetime in seconds. Clamped to
    /// `[MIN_DURATION_SECONDS, MAX_DURATION_SECONDS]`; defaults to
    /// `DEFAULT_DURATION_SECONDS` when omitted.
    #[serde(default)]
    pub duration_seconds: Option<i32>,
}

/// Response body for `POST /v1/credentials/mint`.
#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct MintCredentialsResponse {
    /// Minted platform client configuration (carries the short-lived creds).
    pub client_config: ClientConfig,
    /// Credential expiry as an RFC3339 timestamp (now + clamped duration).
    ///
    /// This is server-computed (`now + clamped duration`), not read back from
    /// the provider — for lazily-resolved configs (e.g. GCP, where the
    /// resolver doesn't always round-trip an authoritative expiry) it is
    /// nominal rather than provider truth. Treat it as a refresh hint: fetch
    /// new credentials at or before this time, don't rely on it to prove the
    /// underlying credential is still valid at that exact instant.
    pub expires_at: String,
    /// Human-readable identity the credentials act as (role ARN, SA email,
    /// managed-identity client id, or `platform:account` for the local path).
    pub principal: String,
}

/// Manual `Debug`: see [`ResolveCredentialsResponse`]'s impl — same reasoning,
/// same secret-bearing field.
impl std::fmt::Debug for MintCredentialsResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MintCredentialsResponse")
            .field("client_config", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("principal", &self.principal)
            .finish()
    }
}

// --- Router ---

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/resolve-credentials", post(resolve_credentials))
        .route("/v1/credentials/mint", post(mint_credentials))
}

// --- Handler ---

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/resolve-credentials",
    tag = "credentials",
    request_body = ResolveCredentialsRequest,
    responses(
        (status = 200, description = "Credentials resolved successfully", body = ResolveCredentialsResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn resolve_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ResolveCredentialsRequest>,
) -> Response {
    let (_subject, deployment) = match authorize_deployment(
        &state,
        &headers,
        &req.deployment_id,
        "resolve credentials for",
    )
    .await
    {
        Ok(pair) => pair,
        Err(response) => return response,
    };

    // Resolve credentials
    match state.credential_resolver.resolve(&deployment).await {
        Ok(client_config) => Json(ResolveCredentialsResponse { client_config }).into_response(),
        Err(e) => e.into_response(),
    }
}

// --- Shared auth/load plumbing ---

/// Load the deployment and authorize the subject on it. Shared by
/// `resolve_credentials` and `mint_credentials`, which both need "valid
/// bearer, deployment exists, subject can act on it" before doing anything
/// endpoint-specific. `action` only affects the forbidden-response wording
/// (e.g. `"mint credentials for"`).
///
/// `can_act_on_deployment` is `can_read_deployment` under the hood: a
/// Workspace/Project-scoped token passes unconditionally, a
/// DeploymentGroup-scoped token passes for any deployment in its group, and a
/// Deployment-scoped token passes only for its own deployment. So a
/// deployment-group token can mint/resolve for every deployment in its group
/// — an inherited grant, not a bug (see the DG-token matrix test).
async fn authorize_deployment(
    state: &AppState,
    headers: &HeaderMap,
    deployment_id: &str,
    action: &str,
) -> std::result::Result<(Subject, DeploymentRecord), Response> {
    let subject = auth::require_auth(state, headers)
        .await
        .map_err(|e| e.into_response())?;

    let deployment = match state
        .deployment_store
        .get_deployment(&subject, deployment_id)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => return Err(ErrorData::not_found_deployment(deployment_id).into_response()),
        Err(e) => return Err(e.into_response()),
    };

    if !state.authz.can_act_on_deployment(&subject, &deployment) {
        return Err(
            ErrorData::forbidden(format!("Cannot {action} this deployment")).into_response(),
        );
    }

    Ok((subject, deployment))
}

// --- Mint handler ---

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/credentials/mint",
    tag = "credentials",
    request_body = MintCredentialsRequest,
    responses(
        (status = 200, description = "Credentials minted successfully", body = MintCredentialsResponse)
    ),
    security(
        ("bearer" = [])
    )
))]
async fn mint_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<MintCredentialsRequest>,
) -> Response {
    // Auth + load + authorize (401 / 404 / 403). See `authorize_deployment`
    // for the scope semantics — notably that DeploymentGroup/Project/Workspace
    // scoped tokens all inherit mint access, not just the deployment's own
    // token.
    let (_subject, deployment) =
        match authorize_deployment(&state, &headers, &req.deployment_id, "mint credentials for")
            .await
        {
            Ok(pair) => pair,
            Err(response) => return response,
        };

    if let Some(response) = validate_sts_session_component("bindingName", &req.binding_name) {
        return response;
    }
    if let Some(response) = validate_sts_session_component("resourceId", &req.resource_id) {
        return response;
    }
    if let Err(response) =
        validate_mint_resource_link(&state, &deployment, &req.resource_id, &req.binding_name).await
    {
        return response;
    }

    let platform = deployment.platform;
    let duration_seconds = clamp_duration(req.duration_seconds);
    let session_name = mint_session_name(&req.deployment_id, &req.resource_id);

    // Cloud minting requires a per-target bindings provider. The generic
    // resolver may contain manager-local static or refreshable credentials and
    // must never be serialized to a workload. Only the Local platform retains
    // resolver fallback for its credential-free fixture config.
    let (client_config, principal, credential_source) =
        if let Some(target_provider) = state.target_bindings_providers.get(&platform) {
            let service_account = match target_provider
                .load_service_account(&req.binding_name)
                .await
            {
                Ok(sa) => sa,
                Err(e) => {
                    return ErrorData::bad_request(format!(
                        "Service-account binding '{}' not available for {}: {}",
                        req.binding_name, platform, e.message
                    ))
                    .into_response()
                }
            };

            // `.context(...)` preserves the source error's code/retryable/
            // http_status_code/chain instead of flattening it into a bare
            // "internal error: {message}" string — same idiom the local
            // (resolver) path gets for free via `e.into_response()`.
            let info = match service_account.get_info().await {
                Ok(info) => info,
                Err(e) => {
                    return e
                        .context(ErrorData::InternalError {
                            message: format!(
                                "Failed to get service-account info for binding '{}'",
                                req.binding_name
                            ),
                        })
                        .into_response()
                }
            };

            let impersonated = match service_account
                .impersonate(ImpersonationRequest {
                    session_name: Some(session_name),
                    duration_seconds: Some(duration_seconds),
                    scopes: None,
                })
                .await
            {
                Ok(config) => config,
                Err(e) => {
                    return e
                        .context(ErrorData::InternalError {
                            message: format!(
                                "Failed to impersonate service-account binding '{}'",
                                req.binding_name
                            ),
                        })
                        .into_response()
                }
            };

            let materialized = match materialize_response_safe_client_config(impersonated).await {
                Ok(config) => config,
                Err(e) => return e.into_response(),
            };
            if materialized.platform() != platform {
                return ErrorData::internal(format!(
                    "Credential impersonation for {platform} returned {} credentials",
                    materialized.platform()
                ))
                .into_response();
            }

            (materialized, principal_from_info(&info), "impersonation")
        } else {
            if platform != Platform::Local {
                return ErrorData::internal(format!(
                    "Credential minting for {platform} requires a target bindings provider"
                ))
                .into_response();
            }

            match state.credential_resolver.resolve(&deployment).await {
                Ok(config @ ClientConfig::Local { .. }) => {
                    let principal = principal_from_client_config(&config);
                    (config, principal, "resolver")
                }
                Ok(_) => {
                    return ErrorData::internal(
                        "Local credential mint fallback returned a non-local client config",
                    )
                    .into_response()
                }
                Err(e) => return e.into_response(),
            }
        };

    let expires_at = (Utc::now() + chrono::Duration::seconds(duration_seconds as i64))
        .to_rfc3339_opts(SecondsFormat::Secs, true);

    // Structured audit trail. NEVER logs credential material — only the
    // request shape, the resolved provider/principal, and the expiry.
    info!(
        deployment_id = %req.deployment_id,
        resource_id = %req.resource_id,
        binding_name = %req.binding_name,
        provider = %platform,
        credential_source = %credential_source,
        principal = %principal,
        expires_at = %expires_at,
        duration_seconds = duration_seconds,
        "Minted deployment credentials"
    );

    Json(MintCredentialsResponse {
        client_config,
        expires_at,
        principal,
    })
    .into_response()
}

// --- Mint helpers ---

/// Ensure an authenticated caller cannot ask for an arbitrary binding. Minting
/// is permitted only for an application compute resource in the deployment's
/// current release, and only for a service account that resource depends on.
async fn validate_mint_resource_link(
    state: &AppState,
    deployment: &DeploymentRecord,
    resource_id: &str,
    binding_name: &str,
) -> std::result::Result<(), Response> {
    let release_id = deployment.current_release_id.as_deref().ok_or_else(|| {
        ErrorData::bad_request("Deployment has no current release; credentials cannot be minted")
            .into_response()
    })?;

    let release = state
        .release_store
        .get_release(&Subject::system(), release_id)
        .await
        .map_err(|error| {
            error
                .context(ErrorData::InternalError {
                    message: format!(
                        "Failed to load current release '{release_id}' for credential minting"
                    ),
                })
                .into_response()
        })?
        .ok_or_else(|| {
            ErrorData::internal(format!(
                "Current release '{release_id}' for deployment '{}' does not exist",
                deployment.id
            ))
            .into_response()
        })?;

    let stack = release.stacks.get(&deployment.platform).ok_or_else(|| {
        ErrorData::internal(format!(
            "Current release '{release_id}' has no {} stack",
            deployment.platform
        ))
        .into_response()
    })?;
    let resource = stack.resources.get(resource_id).ok_or_else(|| {
        ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not part of the deployment's current release"
        ))
        .into_response()
    })?;

    let resource_type = resource.config.resource_type();
    if resource_type != Container::RESOURCE_TYPE
        && resource_type != Worker::RESOURCE_TYPE
        && resource_type != Daemon::RESOURCE_TYPE
    {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' is not an application compute resource"
        ))
        .into_response());
    }

    let intrinsic_dependencies = resource.config.get_dependencies();
    let has_declared_binding_dependency = resource
        .dependencies
        .iter()
        .chain(intrinsic_dependencies.iter())
        .any(|dependency| {
            dependency.resource_type() == &ServiceAccount::RESOURCE_TYPE
                && dependency.id() == binding_name
        });
    // Release records contain the user stack. Deployment preflights
    // deterministically create `{profile}-sa` and add the dependency for app
    // compute resources, so recognize that generated current-release edge
    // without consulting `runtime_metadata.prepared_stack` (which may already
    // describe a desired release during an update).
    let has_generated_binding_dependency = resource
        .config
        .get_permissions()
        .filter(|profile| stack.permissions.profiles.contains_key(*profile))
        .is_some_and(|profile| {
            binding_name == format!("{profile}-sa")
                && !(profile == "management"
                    && matches!(
                        deployment.platform,
                        Platform::Aws | Platform::Gcp | Platform::Azure
                    ))
        });
    let has_binding_dependency =
        has_declared_binding_dependency || has_generated_binding_dependency;
    if !has_binding_dependency {
        return Err(ErrorData::bad_request(format!(
            "Resource '{resource_id}' does not depend on service-account binding '{binding_name}'"
        ))
        .into_response());
    }

    Ok(())
}

/// Convert provider impersonation output into a response-safe credential
/// form. Refreshable sources (service-account keys, workload identity files,
/// managed-identity endpoints, manager profiles, etc.) never cross the API.
pub(super) async fn materialize_response_safe_client_config(
    config: ClientConfig,
) -> std::result::Result<ClientConfig, AlienError<ErrorData>> {
    match config {
        ClientConfig::Aws(config)
            if matches!(
                &config.credentials,
                AwsCredentials::SessionCredentials { .. }
            ) =>
        {
            Ok(ClientConfig::Aws(config))
        }
        ClientConfig::Aws(_) => Err(ErrorData::internal(
            "AWS impersonation did not return short-lived session credentials",
        )),
        ClientConfig::Gcp(config) => {
            let token = config
                .get_bearer_token(GCP_CLOUD_PLATFORM_SCOPE)
                .await
                .context(ErrorData::InternalError {
                    message: "Failed to materialize short-lived GCP credentials".to_string(),
                })?;
            let config = *config;
            Ok(ClientConfig::Gcp(Box::new(alien_core::GcpClientConfig {
                credentials: GcpCredentials::AccessToken { token },
                ..config
            })))
        }
        ClientConfig::Azure(config) => {
            if matches!(&config.credentials, AzureCredentials::AccessToken { .. }) {
                return Err(ErrorData::internal(
                    "Azure impersonation returned a single-scope access token; exact per-scope tokens are required",
                ));
            }
            let mut tokens = HashMap::with_capacity(AZURE_MINT_SCOPES.len());
            for scope in AZURE_MINT_SCOPES {
                let token = config.get_bearer_token_with_scope(scope).await.context(
                    ErrorData::InternalError {
                        message: format!(
                        "Failed to materialize short-lived Azure credentials for scope '{scope}'"
                    ),
                    },
                )?;
                tokens.insert(scope.to_string(), token);
            }
            let config = *config;
            Ok(ClientConfig::Azure(Box::new(
                alien_core::AzureClientConfig {
                    credentials: AzureCredentials::ScopedAccessTokens { tokens },
                    ..config
                },
            )))
        }
        other => Err(ErrorData::internal(format!(
            "Credential impersonation returned unsupported {} client config",
            other.platform()
        ))),
    }
}

/// Clamp a requested duration into the allowed window, defaulting when absent.
fn clamp_duration(requested: Option<i32>) -> i32 {
    requested
        .unwrap_or(DEFAULT_DURATION_SECONDS)
        .clamp(MIN_DURATION_SECONDS, MAX_DURATION_SECONDS)
}

/// Build the impersonation session name `alien-mint-{deploymentId}-{resourceId}`,
/// truncated to fit STS's `RoleSessionName` limit ([`MAX_SESSION_NAME_LEN`]).
///
/// When the raw name is too long, it is replaced by a stable
/// `{prefix}-{hash8}` form so distinct inputs keep distinct (and reproducible)
/// session names while staying within the limit.
fn mint_session_name(deployment_id: &str, resource_id: &str) -> String {
    let raw = format!("alien-mint-{deployment_id}-{resource_id}");
    truncate_session_name(&raw)
}

/// Truncate a session name to [`MAX_SESSION_NAME_LEN`] with a hash suffix.
///
/// The suffix is the first 8 hex chars of the SHA-256 of the full raw name,
/// keeping the result deterministic and collision-resistant.
///
/// Callers today only pass STS-safe-charset (hence ASCII) input — see
/// [`validate_sts_session_component`] — but this walks the cut point back to
/// the nearest `char` boundary regardless, as defense in depth against any
/// future caller that isn't validated the same way. Byte-slicing a
/// multi-byte-straddling index panics; there is no excuse to let that surface
/// here again.
fn truncate_session_name(raw: &str) -> String {
    if raw.len() <= MAX_SESSION_NAME_LEN {
        return raw.to_string();
    }
    let suffix = sha256_hash(raw);
    let suffix = &suffix[..8];
    // prefix + '-' + 8-char suffix == MAX_SESSION_NAME_LEN
    let mut prefix_len = MAX_SESSION_NAME_LEN - 1 - suffix.len();
    while prefix_len > 0 && !raw.is_char_boundary(prefix_len) {
        prefix_len -= 1;
    }
    format!("{}-{}", &raw[..prefix_len], suffix)
}

/// STS `RoleSessionName` safe charset: `[A-Za-z0-9_+=,.@-]`. AWS STS rejects
/// anything outside this set, and folding caller-controlled text into the
/// session name without checking it first is how [`truncate_session_name`]
/// used to panic on non-ASCII input. Validating up front turns that into a
/// clean 400 and forecloses weird strings from ever reaching the audit log.
///
/// Returns `Some(response)` (a 400) when `value` is invalid, `None` when it's
/// fine — an `Option` rather than `Result<(), Response>` because the `Ok`
/// side carries no data and clippy (rightly) flags a `Result` whose only
/// payload is a fat `Response` in the `Err` arm.
fn validate_sts_session_component(field: &str, value: &str) -> Option<Response> {
    let is_safe = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-'));
    if is_safe {
        None
    } else {
        Some(
            ErrorData::bad_request(format!(
                "{field} contains characters outside the STS session-name charset (allowed: A-Z a-z 0-9 _ + = , . @ -)"
            ))
            .into_response(),
        )
    }
}

/// Derive a principal string from an impersonated service account's identity.
fn principal_from_info(info: &ServiceAccountInfo) -> String {
    match info {
        ServiceAccountInfo::Aws(aws) => aws.role_arn.clone(),
        ServiceAccountInfo::Gcp(gcp) => gcp.email.clone(),
        ServiceAccountInfo::Azure(azure) => azure.client_id.clone(),
    }
}

/// Derive a principal string from resolved (non-impersonated) credentials.
fn principal_from_client_config(config: &ClientConfig) -> String {
    match config {
        ClientConfig::Aws(aws) => format!("aws:{}", aws.account_id),
        ClientConfig::Gcp(gcp) => format!("gcp:{}", gcp.project_id),
        ClientConfig::Azure(azure) => format!("azure:{}", azure.subscription_id),
        other => other.platform().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_duration, mint_session_name, principal_from_client_config, principal_from_info,
        truncate_session_name, MintCredentialsResponse, ResolveCredentialsResponse,
        MAX_SESSION_NAME_LEN,
    };
    use alien_bindings::ServiceAccountInfo;
    use alien_bindings::{
        traits::AwsServiceAccountInfo, traits::AzureServiceAccountInfo,
        traits::GcpServiceAccountInfo,
    };
    use alien_core::{AwsClientConfig, AwsCredentials, ClientConfig};

    #[test]
    fn clamp_duration_defaults_when_absent() {
        assert_eq!(clamp_duration(None), 3600);
    }

    #[test]
    fn clamp_duration_clamps_below_minimum() {
        assert_eq!(clamp_duration(Some(10)), 900);
        assert_eq!(clamp_duration(Some(-5)), 900);
    }

    #[test]
    fn clamp_duration_clamps_above_maximum() {
        assert_eq!(clamp_duration(Some(100_000)), 3600);
    }

    #[test]
    fn clamp_duration_passes_through_in_range() {
        assert_eq!(clamp_duration(Some(1800)), 1800);
        assert_eq!(clamp_duration(Some(900)), 900);
        assert_eq!(clamp_duration(Some(3600)), 3600);
    }

    #[test]
    fn session_name_short_is_unchanged() {
        assert_eq!(
            mint_session_name("dep_123", "api"),
            "alien-mint-dep_123-api"
        );
    }

    #[test]
    fn session_name_truncates_long_input_within_limit() {
        let long_deployment = "d".repeat(80);
        let name = mint_session_name(&long_deployment, "some-long-resource-name");
        assert!(
            name.len() <= MAX_SESSION_NAME_LEN,
            "session name must fit the STS limit, got {} chars: {name}",
            name.len()
        );
        assert!(name.starts_with("alien-mint-"));
    }

    #[test]
    fn session_name_truncation_is_deterministic_and_distinct() {
        let base = "e".repeat(80);
        let a = mint_session_name(&base, "resource-a");
        let b = mint_session_name(&base, "resource-b");
        // Same input -> same output.
        assert_eq!(a, mint_session_name(&base, "resource-a"));
        // Different input -> different output (hash suffix disambiguates).
        assert_ne!(a, b);
        assert!(a.len() <= MAX_SESSION_NAME_LEN && b.len() <= MAX_SESSION_NAME_LEN);
    }

    #[test]
    fn truncate_session_name_exactly_at_limit_is_unchanged() {
        let exact = "x".repeat(MAX_SESSION_NAME_LEN);
        assert_eq!(truncate_session_name(&exact), exact);
    }

    #[test]
    fn truncate_session_name_multibyte_input_does_not_panic_on_boundary() {
        // 'é' is 2 bytes in UTF-8. 40 repetitions is an 80-byte string whose
        // deterministic prefix cut (byte 55, per MAX_SESSION_NAME_LEN) lands
        // mid-character. This must not panic regardless of what upstream
        // validation does — defense in depth for future callers of this
        // helper.
        let raw = "é".repeat(40);
        let truncated = truncate_session_name(&raw);
        assert!(
            truncated.len() <= MAX_SESSION_NAME_LEN,
            "got {} bytes: {truncated}",
            truncated.len()
        );
    }

    #[test]
    fn principal_from_info_extracts_platform_identity() {
        assert_eq!(
            principal_from_info(&ServiceAccountInfo::Aws(AwsServiceAccountInfo {
                role_name: "r".to_string(),
                role_arn: "arn:aws:iam::123:role/r".to_string(),
            })),
            "arn:aws:iam::123:role/r"
        );
        assert_eq!(
            principal_from_info(&ServiceAccountInfo::Gcp(GcpServiceAccountInfo {
                email: "sa@project.iam.gserviceaccount.com".to_string(),
                unique_id: "1".to_string(),
            })),
            "sa@project.iam.gserviceaccount.com"
        );
        assert_eq!(
            principal_from_info(&ServiceAccountInfo::Azure(AzureServiceAccountInfo {
                client_id: "client-abc".to_string(),
                resource_id: "/id".to_string(),
                principal_id: "p".to_string(),
            })),
            "client-abc"
        );
    }

    #[test]
    fn mint_response_debug_redacts_client_config() {
        let secret_config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIA_SECRET".to_string(),
                secret_access_key: "TOP_SECRET_KEY_MATERIAL".to_string(),
                session_token: Some("TOP_SECRET_SESSION_TOKEN".to_string()),
            },
            service_overrides: None,
        }));

        let mint_response = MintCredentialsResponse {
            client_config: secret_config.clone(),
            expires_at: "2026-01-01T00:00:00Z".to_string(),
            principal: "arn:aws:iam::123:role/r".to_string(),
        };
        let mint_debug = format!("{:?}", mint_response);
        assert!(
            mint_debug.contains("<redacted>"),
            "expected redaction marker: {mint_debug}"
        );
        assert!(!mint_debug.contains("TOP_SECRET_KEY_MATERIAL"));
        assert!(!mint_debug.contains("TOP_SECRET_SESSION_TOKEN"));

        let resolve_response = ResolveCredentialsResponse {
            client_config: secret_config,
        };
        let resolve_debug = format!("{:?}", resolve_response);
        assert!(
            resolve_debug.contains("<redacted>"),
            "expected redaction marker: {resolve_debug}"
        );
        assert!(!resolve_debug.contains("TOP_SECRET_KEY_MATERIAL"));
        assert!(!resolve_debug.contains("TOP_SECRET_SESSION_TOKEN"));
    }

    #[test]
    fn principal_from_client_config_uses_account_scope() {
        let config = ClientConfig::Aws(Box::new(AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIA".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: None,
            },
            service_overrides: None,
        }));
        assert_eq!(principal_from_client_config(&config), "aws:123456789012");
    }
}
