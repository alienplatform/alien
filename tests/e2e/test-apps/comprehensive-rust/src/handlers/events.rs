//! Event verification handlers for testing.
//!
//! These handlers allow tests to verify that events were received and processed
//! by retrieving stored event data from KV.

use axum::{
    extract::{Path, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{models::AppState, ErrorData, Result};
use alien_error::Context;

/// Response for a single event lookup
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventResponse {
    pub found: bool,
    pub event: Option<serde_json::Value>,
}

/// Response for listing all events
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventListResponse {
    pub storage_events: Vec<serde_json::Value>,
    pub cron_events: Vec<serde_json::Value>,
    pub queue_messages: Vec<serde_json::Value>,
}

/// Get a stored storage event by key
pub async fn get_storage_event(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<EventResponse>> {
    info!(key = %key, "Looking up storage event");

    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("alien-kv")
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: "alien-kv".to_string(),
        })?;

    // Sanitize key: replace / with _ to match how events are stored
    let sanitized_key = key.replace('/', "_");
    let kv_key = format!("storage_event:{}", sanitized_key);
    match kv.get(&kv_key).await {
        Ok(Some(data)) => {
            let event: serde_json::Value = serde_json::from_slice(&data).unwrap_or_default();
            Ok(Json(EventResponse {
                found: true,
                event: Some(event),
            }))
        }
        _ => Ok(Json(EventResponse {
            found: false,
            event: None,
        })),
    }
}

/// Get a stored cron event by schedule name
pub async fn get_cron_event(
    State(app_state): State<AppState>,
    Path(schedule): Path<String>,
) -> Result<Json<EventResponse>> {
    info!(schedule = %schedule, "Looking up cron event");

    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("alien-kv")
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: "alien-kv".to_string(),
        })?;

    // Sanitize schedule: replace / with _ to match how events are stored
    let sanitized_schedule = schedule.replace('/', "_");
    let kv_key = format!("cron_event:{}", sanitized_schedule);
    match kv.get(&kv_key).await {
        Ok(Some(data)) => {
            let event: serde_json::Value = serde_json::from_slice(&data).unwrap_or_default();
            Ok(Json(EventResponse {
                found: true,
                event: Some(event),
            }))
        }
        _ => Ok(Json(EventResponse {
            found: false,
            event: None,
        })),
    }
}

/// Get a stored queue message by ID
pub async fn get_queue_message(
    State(app_state): State<AppState>,
    Path(message_id): Path<String>,
) -> Result<Json<EventResponse>> {
    info!(message_id = %message_id, "Looking up queue message");

    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("alien-kv")
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: "alien-kv".to_string(),
        })?;

    // Sanitize message ID: replace / with _ to match how events are stored
    let sanitized_id = message_id.replace('/', "_");
    let kv_key = format!("queue_message:{}", sanitized_id);
    match kv.get(&kv_key).await {
        Ok(Some(data)) => {
            let event: serde_json::Value = serde_json::from_slice(&data).unwrap_or_default();
            Ok(Json(EventResponse {
                found: true,
                event: Some(event),
            }))
        }
        _ => Ok(Json(EventResponse {
            found: false,
            event: None,
        })),
    }
}

/// List all stored events
pub async fn list_events(State(app_state): State<AppState>) -> Result<Json<EventListResponse>> {
    info!("Listing all stored events");

    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("alien-kv")
        .await
        .context(ErrorData::BindingNotFound {
            binding_name: "alien-kv".to_string(),
        })?;

    let mut storage_events = Vec::new();
    let mut cron_events = Vec::new();
    let mut queue_messages = Vec::new();

    // Scan for storage events
    if let Ok(result) = kv.scan_prefix("storage_event:", Some(100), None).await {
        for (_, value) in result.items {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                storage_events.push(event);
            }
        }
    }

    // Scan for cron events
    if let Ok(result) = kv.scan_prefix("cron_event:", Some(100), None).await {
        for (_, value) in result.items {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                cron_events.push(event);
            }
        }
    }

    // Scan for queue messages
    if let Ok(result) = kv.scan_prefix("queue_message:", Some(100), None).await {
        for (_, value) in result.items {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&value) {
                queue_messages.push(event);
            }
        }
    }

    info!(
        storage_count = storage_events.len(),
        cron_count = cron_events.len(),
        queue_count = queue_messages.len(),
        "Listed events"
    );

    Ok(Json(EventListResponse {
        storage_events,
        cron_events,
        queue_messages,
    }))
}
