//! `/v1/values/{scope}` — deployment values held by this manager.
//!
//! Callers write values here directly, so the service that tracks the
//! deployment only ever sees metadata: the slot, its kind, when it changed and
//! a keyed digest. Secret values are never returned.

use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, State},
    http::{header::IF_NONE_MATCH, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::error::{ErrorData, Result};
use crate::values::{
    ScopeKind, SlotSelection, ValueAccess, ValueAccessAuthorizer, ValueGrant, ValueKind, ValueMeta,
    ValueScope, ValueSlot, ValueStore,
};

/// Most values one PUT or verify request may carry.
pub const MAX_VALUES_PER_REQUEST: usize = 100;

/// Largest PUT or verify request body, in bytes (8 MiB). 100 values of 24,000
/// bytes fit even when escaping doubles their size.
pub const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/v1/values/{scope}",
            get(list_values)
                .put(put_values)
                .delete(delete_scope)
                .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES)),
        )
        .route(
            "/v1/values/{scope}/verify",
            post(verify_values).layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES)),
        )
        .route(
            "/v1/values/{scope}/export",
            post(export_values).layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES)),
        )
        .route("/v1/values/{scope}/move", post(move_values))
        .route("/v1/values/{scope}/{slot}", delete(delete_value))
}

/// The request body, or the error to answer with. A body over the limit
/// gets a 413 that states the limits; a body with a malformed value, such as
/// a slot outside the grammar, gets a 400.
fn read_body<T>(
    body: std::result::Result<Json<T>, JsonRejection>,
) -> std::result::Result<T, Response> {
    match body {
        Ok(Json(body)) => Ok(body),
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(request_too_large().into_response())
        }
        Err(rejection) if rejection.status() == StatusCode::UNPROCESSABLE_ENTITY => {
            Err(ErrorData::bad_request(rejection.body_text()).into_response())
        }
        Err(rejection) => Err(rejection.into_response()),
    }
}

fn request_too_large() -> alien_error::AlienError<ErrorData> {
    alien_error::AlienError::new(ErrorData::ValuesRequestTooLarge {
        reason: format!(
            "A request may carry at most {MAX_VALUES_PER_REQUEST} values and 8 MiB; \
             send fewer values per request"
        ),
    })
}

fn check_item_count(count: usize) -> Result<()> {
    if count > MAX_VALUES_PER_REQUEST {
        return Err(request_too_large());
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PutValuesRequest {
    pub values: Vec<PutValue>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PutValue {
    /// `env:<NAME>`, `input:<id>` or `cenv.<containerName>.<NAME>`.
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub slot: ValueSlot,
    pub value: String,
    pub kind: ValueKind,
}

impl std::fmt::Debug for PutValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PutValue")
            .field("slot", &self.slot)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ValueSlotsResponse {
    pub slots: Vec<ValueSlotEntry>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ValueSlotEntry {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub slot: ValueSlot,
    pub kind: ValueKind,
    /// Keyed digest of the value, lowercase hex.
    pub digest: String,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// The value, for plain slots only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

impl From<ValueMeta> for ValueSlotEntry {
    fn from(meta: ValueMeta) -> Self {
        Self {
            slot: meta.slot,
            kind: meta.kind,
            digest: meta.digest,
            updated_at: meta.updated_at,
            value: None,
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VerifyValuesRequest {
    pub values: Vec<VerifyValue>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VerifyValue {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub slot: ValueSlot,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VerifyValuesResponse {
    pub results: Vec<VerifyResult>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub slot: ValueSlot,
    #[serde(rename = "match")]
    pub matches: bool,
}

struct Values<'a> {
    store: &'a ValueStore,
    authorizer: &'a dyn ValueAccessAuthorizer,
}

fn values(state: &AppState) -> Result<Values<'_>> {
    match (&state.values, &state.value_access) {
        (Some(store), Some(authorizer)) => Ok(Values {
            store,
            authorizer: authorizer.as_ref(),
        }),
        _ => Err(alien_error::AlienError::new(ErrorData::ValuesUnavailable {
            reason: "no value store is configured".to_string(),
        })),
    }
}

async fn authorize(
    values: &Values<'_>,
    headers: &HeaderMap,
    scope: &ValueScope,
    needed: ValueAccess,
    selection: &SlotSelection,
) -> Result<ValueGrant> {
    let grant = values.authorizer.authorize(headers, scope, needed).await?;
    grant.check(needed, selection)?;
    Ok(grant)
}

/// Whether the request asks to write only into empty slots. Only the
/// `If-None-Match: *` form is understood; any other value is refused rather
/// than ignored, so a caller never believes it got a guard it did not.
fn only_if_absent(headers: &HeaderMap) -> Result<bool> {
    let Some(header) = headers.get(IF_NONE_MATCH) else {
        return Ok(false);
    };
    if header.as_bytes() == b"*" {
        return Ok(true);
    }
    Err(ErrorData::bad_request(
        "If-None-Match on a values PUT must be '*'".to_string(),
    ))
}

/// Headers of a values PUT. Only documents the condition; the handler reads it from the raw headers.
#[cfg(feature = "openapi")]
#[derive(utoipa::IntoParams)]
#[into_params(parameter_in = Header)]
#[allow(dead_code)]
struct PutValuesHeaders {
    /// `*` writes only when the slot is empty
    #[param(rename = "If-None-Match", nullable = false)]
    if_none_match: Option<String>,
}

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/v1/values/{scope}",
    operation_id = "put_values",
    tag = "values",
    description = "Stores the values, replacing any held in the same slots. With the header `If-None-Match: *` the request must carry exactly one value, and it is written only when its slot is empty, atomically: of several concurrent conditional writes to one slot exactly one succeeds. A manager that serves values (`capabilities.values` = 1) honours this header.",
    params(
        ("scope" = String, Path, description = "dep:<deploymentId>"),
        PutValuesHeaders
    ),
    request_body = PutValuesRequest,
    responses(
        (status = 200, description = "Metadata of the stored values", body = ValueSlotsResponse),
        (status = 400, description = "With `If-None-Match: *`, the request does not carry exactly one value (VALUES_CONDITION_SINGLE_SLOT)"),
        (status = 403, description = "A slot is outside the caller's access"),
        (status = 412, description = "With `If-None-Match: *`, the slot already holds a value; nothing was written (VALUE_ALREADY_EXISTS)"),
        (status = 413, description = "A value is larger than 24,000 bytes, or the request carries more than 100 values or 8 MiB")
    ),
    security(("bearer" = []))
))]
pub(crate) async fn put_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    body: std::result::Result<Json<PutValuesRequest>, JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        scope: &str,
        request: PutValuesRequest,
    ) -> Result<ValueSlotsResponse> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        let selection =
            SlotSelection::Slots(request.values.iter().map(|v| v.slot.clone()).collect());
        authorize(&values, headers, &scope, ValueAccess::Write, &selection).await?;
        check_item_count(request.values.len())?;
        let only_if_absent = only_if_absent(headers)?;
        if only_if_absent && request.values.len() != 1 {
            return Err(alien_error::AlienError::new(
                ErrorData::ValuesConditionSingleSlot {
                    count: request.values.len(),
                },
            ));
        }

        // Check every value before writing any, so a rejected batch changes nothing.
        for value in &request.values {
            crate::values::check_incoming(&value.slot, &value.value)?;
        }
        let mut slots = Vec::with_capacity(request.values.len());
        for value in request.values {
            let meta = if only_if_absent {
                values
                    .store
                    .put_if_absent(&scope, &value.slot, value.kind, value.value)
                    .await?
                    .ok_or_else(|| {
                        alien_error::AlienError::new(ErrorData::ValueAlreadyExists {
                            scope: scope.to_string(),
                            slot: value.slot.to_string(),
                        })
                    })?
            } else {
                values
                    .store
                    .put(&scope, &value.slot, value.kind, value.value)
                    .await?
            };
            slots.push(meta.into());
        }
        Ok(ValueSlotsResponse { slots })
    }
    let request = match read_body(body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match run(&state, &headers, &scope, request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    get,
    path = "/v1/values/{scope}",
    operation_id = "list_values",
    tag = "values",
    params(("scope" = String, Path, description = "dep:<deploymentId>")),
    responses((status = 200, description = "Metadata of the stored values, with plain values", body = ValueSlotsResponse)),
    security(("bearer" = []))
))]
pub(crate) async fn list_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
) -> Response {
    async fn run(state: &AppState, headers: &HeaderMap, scope: &str) -> Result<ValueSlotsResponse> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        // A caller with a slot list reads those slots; one without reads the scope.
        let grant = values
            .authorizer
            .authorize(headers, &scope, ValueAccess::Read)
            .await?;
        grant.check_access(ValueAccess::Read)?;

        let mut slots = Vec::new();
        for stored in values.store.list(&scope).await? {
            if !grant.allows_slot(&stored.slot) {
                continue;
            }
            let mut entry = ValueSlotEntry::from(stored.meta(&scope)?);
            if stored.kind == ValueKind::Plain {
                entry.value = Some(stored.value);
            }
            slots.push(entry);
        }
        Ok(ValueSlotsResponse { slots })
    }
    match run(&state, &headers, &scope).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/v1/values/{scope}/{slot}",
    operation_id = "delete_value",
    tag = "values",
    params(
        ("scope" = String, Path, description = "dep:<deploymentId>"),
        ("slot" = String, Path, description = "env:<NAME>, input:<id> or cenv.<containerName>.<NAME>")
    ),
    responses((status = 204, description = "Deleted, or there was nothing to delete")),
    security(("bearer" = []))
))]
pub(crate) async fn delete_value(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope, slot)): Path<(String, String)>,
) -> Response {
    async fn run(state: &AppState, headers: &HeaderMap, scope: &str, slot: &str) -> Result<()> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        let slot: ValueSlot = slot.parse()?;
        authorize(
            &values,
            headers,
            &scope,
            ValueAccess::Write,
            &SlotSelection::Slots(vec![slot.clone()]),
        )
        .await?;
        values.store.delete(&scope, &slot).await
    }
    match run(&state, &headers, &scope, &slot).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/values/{scope}/verify",
    operation_id = "verify_values",
    tag = "values",
    params(("scope" = String, Path, description = "dep:<deploymentId>")),
    request_body = VerifyValuesRequest,
    responses(
        (status = 200, description = "Whether each value equals the stored one", body = VerifyValuesResponse),
        (status = 400, description = "A slot appears more than once in the request"),
        (status = 429, description = "Too many verifications for one of the slots")
    ),
    security(("bearer" = []))
))]
// Migration only: lets a control plane that moves values compare them
// without reading them. Removed in Deploy 2.
pub(crate) async fn verify_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    body: std::result::Result<Json<VerifyValuesRequest>, JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        scope: &str,
        request: VerifyValuesRequest,
    ) -> Result<VerifyValuesResponse> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        let slots: Vec<ValueSlot> = request.values.iter().map(|v| v.slot.clone()).collect();
        let selection = SlotSelection::Slots(slots.clone());
        authorize(&values, headers, &scope, ValueAccess::Migrate, &selection).await?;
        check_item_count(request.values.len())?;
        let mut distinct = std::collections::HashSet::new();
        if let Some(repeated) = slots.iter().find(|slot| !distinct.insert(*slot)) {
            return Err(ErrorData::bad_request(format!(
                "The slot '{repeated}' appears more than once in one verify request"
            )));
        }
        if !state.verify_limiter.allow(&scope, &slots) {
            return Err(alien_error::AlienError::new(
                ErrorData::ValueVerifyRateLimited {
                    scope: scope.to_string(),
                },
            ));
        }
        let mut results = Vec::with_capacity(request.values.len());
        for candidate in request.values {
            let matches = values
                .store
                .matches(&scope, &candidate.slot, &candidate.value)
                .await?;
            results.push(VerifyResult {
                slot: candidate.slot,
                matches,
            });
        }
        Ok(VerifyValuesResponse { results })
    }
    let request = match read_body(body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match run(&state, &headers, &scope, request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ExportValuesRequest {
    /// Base URL of the manager that takes the values. The caller's token names the one URL
    /// allowed; any other is refused.
    pub target_url: String,
    /// Token the target accepts for writing these slots. Passed on as is.
    pub target_token: String,
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<String>))]
    pub slots: Vec<ValueSlot>,
}

impl std::fmt::Debug for ExportValuesRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportValuesRequest")
            .field("target_url", &self.target_url)
            .field("slots", &self.slots)
            .finish_non_exhaustive()
    }
}

/// Plain http is accepted only for a loopback host, which never leaves the machine.
fn export_target_allowed(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => true,
        "http" => parsed.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(|c| c == '[' || c == ']')
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        }),
        _ => false,
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    delete,
    path = "/v1/values/{scope}",
    operation_id = "delete_scope",
    tag = "values",
    description = "Removes every value of a setup link or registration scope, for when the link is revoked or the registration expires. A deployment scope is refused: its values go with the deployment.",
    params(("scope" = String, Path, description = "link:<setupLinkId> or reg:<registrationId>")),
    responses((status = 204, description = "Deleted, or there was nothing to delete")),
    security(("bearer" = []))
))]
pub(crate) async fn delete_scope(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
) -> Response {
    async fn run(state: &AppState, headers: &HeaderMap, scope: &str) -> Result<()> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        if scope.kind() == ScopeKind::Deployment {
            return Err(ErrorData::bad_request(
                "Delete a deployment's values slot by slot".to_string(),
            ));
        }
        authorize(
            &values,
            headers,
            &scope,
            ValueAccess::Write,
            &SlotSelection::All,
        )
        .await?;
        values.store.delete_scope(&scope).await
    }
    match run(&state, &headers, &scope).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/values/{scope}/export",
    operation_id = "export_values",
    tag = "values",
    description = "Sends the stored values of the named slots to another manager, which writes them with the token given. The values go from this manager to the target; the response carries only the target's metadata. The caller's token must be an export token naming the target URL, and the request may name no other.",
    params(("scope" = String, Path, description = "dep:<deploymentId>")),
    request_body = ExportValuesRequest,
    responses(
        (status = 200, description = "The target's metadata of the stored values", body = ValueSlotsResponse),
        (status = 403, description = "The token is not an export token, the target URL is not the one it names, or a slot is outside it"),
        (status = 404, description = "A named slot holds no value here"),
        (status = 502, description = "The target refused or could not be reached")
    ),
    security(("bearer" = []))
))]
pub(crate) async fn export_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    body: std::result::Result<Json<ExportValuesRequest>, JsonRejection>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        scope: &str,
        request: ExportValuesRequest,
    ) -> Result<ValueSlotsResponse> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        let selection = SlotSelection::Slots(request.slots.clone());
        let grant = authorize(&values, headers, &scope, ValueAccess::Export, &selection).await?;
        let named = grant.export_target.as_deref().ok_or_else(|| {
            ErrorData::forbidden("This token does not name a manager to export to".to_string())
        })?;
        if named.trim_end_matches('/') != request.target_url.trim_end_matches('/') {
            return Err(ErrorData::forbidden(
                "The target is not the manager this token names".to_string(),
            ));
        }
        if !export_target_allowed(named) {
            return Err(ErrorData::forbidden(
                "Values are exported only to an https URL".to_string(),
            ));
        }
        check_item_count(request.slots.len())?;
        let mut distinct = std::collections::HashSet::new();
        if let Some(repeated) = request.slots.iter().find(|slot| !distinct.insert(*slot)) {
            return Err(ErrorData::bad_request(format!(
                "The slot '{repeated}' appears more than once in one export request"
            )));
        }

        // Read every slot before sending any, so a missing one sends nothing.
        let mut outgoing = Vec::with_capacity(request.slots.len());
        for slot in &request.slots {
            let stored = values.store.get(&scope, slot).await?.ok_or_else(|| {
                alien_error::AlienError::new(ErrorData::ValueExportSlotMissing {
                    slot: slot.to_string(),
                })
            })?;
            outgoing.push(serde_json::json!({
                "slot": slot.to_string(),
                "value": stored.value,
                "kind": stored.kind,
            }));
        }

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ErrorData::internal(e.to_string()))?;
        let url = format!("{}/v1/values/{}", named.trim_end_matches('/'), scope);
        let response = client
            .put(&url)
            .bearer_auth(&request.target_token)
            .json(&serde_json::json!({ "values": outgoing }))
            .send()
            .await
            .map_err(|_| export_failed("the target could not be reached".to_string()))?;
        if !response.status().is_success() {
            return Err(export_failed(format!(
                "the target answered {}",
                response.status().as_u16()
            )));
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TargetSlot {
            slot: ValueSlot,
            kind: ValueKind,
            digest: String,
            updated_at: chrono::DateTime<chrono::Utc>,
        }
        #[derive(Deserialize)]
        struct TargetSlots {
            slots: Vec<TargetSlot>,
        }
        let stored: TargetSlots = response
            .json()
            .await
            .map_err(|_| export_failed("the target's answer could not be read".to_string()))?;
        Ok(ValueSlotsResponse {
            slots: stored
                .slots
                .into_iter()
                .map(|s| ValueSlotEntry {
                    slot: s.slot,
                    kind: s.kind,
                    digest: s.digest,
                    updated_at: s.updated_at,
                    value: None,
                })
                .collect(),
        })
    }
    let request = match read_body(body) {
        Ok(request) => request,
        Err(response) => return response,
    };
    match run(&state, &headers, &scope, request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

#[cfg_attr(feature = "openapi", utoipa::path(
    post,
    path = "/v1/values/{scope}/move",
    operation_id = "move_values",
    tag = "values",
    description = "Copies a setup link's values, or moves a registration's values, into the deployment created from it. Needs a signed claim that names the source and the deployment; nothing in the request body or path chooses the destination. Repeating a finished move changes nothing.",
    params(("scope" = String, Path, description = "link:<setupLinkId> or reg:<registrationId>")),
    responses(
        (status = 200, description = "Metadata of the values now held by the deployment", body = ValueSlotsResponse),
        (status = 403, description = "The claim does not cover this move")
    ),
    security(("bearer" = []))
))]
pub(crate) async fn move_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
) -> Response {
    async fn run(state: &AppState, headers: &HeaderMap, scope: &str) -> Result<ValueSlotsResponse> {
        let values = values(state)?;
        let source: ValueScope = scope.parse()?;
        if source.kind() == ScopeKind::Deployment {
            return Err(ErrorData::bad_request(
                "Only the values of a setup link or registration can be moved".to_string(),
            ));
        }
        let claim = values.authorizer.authorize_move(headers, &source).await?;
        // The authorizer must answer for exactly this source and a deployment scope.
        if claim.from != source || claim.to.kind() != ScopeKind::Deployment {
            return Err(ErrorData::forbidden("This claim does not cover this move"));
        }
        // A setup link keeps its values for the next deployment; a registration
        // creates one deployment, so its values move.
        let moved = if source.kind() == ScopeKind::SetupLink {
            values
                .store
                .copy_scope(&claim.from, &claim.to, &claim.claim_id)
                .await?
        } else {
            values
                .store
                .move_scope(&claim.from, &claim.to, &claim.claim_id)
                .await?
        };
        Ok(ValueSlotsResponse {
            slots: moved.into_iter().map(Into::into).collect(),
        })
    }
    match run(&state, &headers, &scope).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}

fn export_failed(reason: String) -> alien_error::AlienError<ErrorData> {
    alien_error::AlienError::new(ErrorData::ValueExportFailed { reason })
}
