//! `/v1/values/{scope}` — deployment values held by this manager.
//!
//! Callers write values here directly, so the service that tracks the
//! deployment only ever sees metadata: the slot, its kind, when it changed and
//! a keyed digest. Secret values are never returned.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::error::{ErrorData, Result};
use crate::values::{
    SlotSelection, ValueAccess, ValueAccessAuthorizer, ValueGrant, ValueKind, ValueMeta,
    ValueScope, ValueSlot, ValueStore,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/values/{scope}", get(list_values).put(put_values))
        .route("/v1/values/{scope}/verify", post(verify_values))
        .route("/v1/values/{scope}/{slot}", delete(delete_value))
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
    /// `env:<NAME>` or `input:<id>`.
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

#[cfg_attr(feature = "openapi", utoipa::path(
    put,
    path = "/v1/values/{scope}",
    operation_id = "put_values",
    tag = "values",
    params(("scope" = String, Path, description = "dep:<id>, link:<id> or reg:<id>")),
    request_body = PutValuesRequest,
    responses(
        (status = 200, description = "Metadata of the stored values", body = ValueSlotsResponse),
        (status = 403, description = "A slot is outside the caller's access"),
        (status = 413, description = "A value is larger than 24,000 bytes")
    ),
    security(("bearer" = []))
))]
pub(crate) async fn put_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    Json(request): Json<PutValuesRequest>,
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

        // Check every value before writing any, so a rejected batch changes nothing.
        for value in &request.values {
            crate::values::check_incoming(&value.slot, &value.value)?;
        }
        let mut slots = Vec::with_capacity(request.values.len());
        for value in request.values {
            let meta = values
                .store
                .put(&scope, &value.slot, value.kind, value.value)
                .await?;
            slots.push(meta.into());
        }
        Ok(ValueSlotsResponse { slots })
    }
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
    params(("scope" = String, Path, description = "dep:<id>, link:<id> or reg:<id>")),
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
            let mut entry = ValueSlotEntry::from(stored.meta()?);
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
        ("scope" = String, Path, description = "dep:<id>, link:<id> or reg:<id>"),
        ("slot" = String, Path, description = "env:<NAME> or input:<id>")
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
    params(("scope" = String, Path, description = "dep:<id>, link:<id> or reg:<id>")),
    request_body = VerifyValuesRequest,
    responses(
        (status = 200, description = "Whether each value equals the stored one", body = VerifyValuesResponse),
        (status = 429, description = "Too many verifications for this scope")
    ),
    security(("bearer" = []))
))]
pub(crate) async fn verify_values(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    Json(request): Json<VerifyValuesRequest>,
) -> Response {
    async fn run(
        state: &AppState,
        headers: &HeaderMap,
        scope: &str,
        request: VerifyValuesRequest,
    ) -> Result<VerifyValuesResponse> {
        let values = values(state)?;
        let scope: ValueScope = scope.parse()?;
        let selection =
            SlotSelection::Slots(request.values.iter().map(|v| v.slot.clone()).collect());
        authorize(&values, headers, &scope, ValueAccess::Migrate, &selection).await?;
        if !state.verify_limiter.allow(
            &scope,
            u32::try_from(request.values.len()).unwrap_or(u32::MAX),
        ) {
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
    match run(&state, &headers, &scope, request).await {
        Ok(response) => Json(response).into_response(),
        Err(e) => e.into_response(),
    }
}
