use crate::error::{ErrorData, Result};
use crate::ui::{make_table, print_table, status_cell};
use alien_error::{Context, IntoAlienError};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDisplayRow {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub state: String,
    pub actor: String,
    pub event: String,
    pub details: String,
    pub summary: String,
}

impl EventDisplayRow {
    pub fn try_new<D, S>(id: String, created_at: DateTime<Utc>, data: &D, state: &S) -> Result<Self>
    where
        D: Serialize,
        S: Serialize,
    {
        let data = serde_json::to_value(data)
            .into_alien_error()
            .context(ErrorData::JsonError {
                operation: "format event data".to_string(),
                reason: "event data could not be serialized".to_string(),
            })?;
        let state =
            serde_json::to_value(state)
                .into_alien_error()
                .context(ErrorData::JsonError {
                    operation: "format event state".to_string(),
                    reason: "event state could not be serialized".to_string(),
                })?;

        let event = event_title(&data, &state);
        let details = event_details(&data, &state);
        let summary = if details.is_empty() {
            event.clone()
        } else {
            format!("{event}: {details}")
        };

        Ok(Self {
            id,
            created_at,
            state: event_state(&state),
            actor: event_actor(&data),
            event,
            details,
            summary,
        })
    }
}

pub fn print_event_table(rows: &[EventDisplayRow]) {
    if rows.is_empty() {
        println!("(no events)");
        return;
    }

    let mut table = make_table(&["Time", "State", "Actor", "Event", "Details"]);
    for row in rows {
        table.add_row(vec![
            row.created_at
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string()
                .into(),
            status_cell(&row.state),
            row.actor.clone().into(),
            row.event.clone().into(),
            row.details.clone().into(),
        ]);
    }
    print_table(table);
}

pub fn print_event_lines(rows: &[EventDisplayRow]) {
    for row in rows {
        let details = if row.details.is_empty() {
            String::new()
        } else {
            format!(" — {}", row.details)
        };
        println!(
            "[{}] {} {}{} ({})",
            row.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            row.actor,
            row.event,
            details,
            row.state
        );
    }
}

fn event_state(state: &Value) -> String {
    match state {
        Value::String(value) => value.clone(),
        Value::Object(object) if object.contains_key("failed") => "failed".to_string(),
        _ => "unknown".to_string(),
    }
}

fn event_actor(data: &Value) -> String {
    let Some(actor) = data.get("actor").and_then(Value::as_object) else {
        return if is_user_intent(data) {
            "Unknown".to_string()
        } else {
            "System".to_string()
        };
    };

    match actor.get("kind").and_then(Value::as_str) {
        Some("user") => actor
            .get("email")
            .and_then(Value::as_str)
            .or_else(|| actor.get("id").and_then(Value::as_str))
            .unwrap_or("Unknown user")
            .to_string(),
        Some("serviceAccount") => actor
            .get("id")
            .and_then(Value::as_str)
            .map(|id| format!("Service account {id}"))
            .unwrap_or_else(|| "Service account".to_string()),
        _ => "System".to_string(),
    }
}

fn is_user_intent(data: &Value) -> bool {
    matches!(
        data.get("type").and_then(Value::as_str),
        Some(
            "ReleaseChannelUpdated"
                | "DeploymentReleaseChannelChanged"
                | "DeploymentRetryRequested"
                | "DeploymentRedeployRequested"
                | "DeploymentReleasePinned"
                | "DeploymentReleaseUnpinned"
                | "DeploymentEnvironmentUpdated"
                | "DeploymentDeletionRequested"
        )
    )
}

fn event_title(data: &Value, state: &Value) -> String {
    let event_type = data.get("type").and_then(Value::as_str).unwrap_or("Event");
    let state = event_state(state);

    match (event_type, state.as_str()) {
        ("DeploymentEnvironmentUpdated", "failed") => "Configuration Update Failed".to_string(),
        ("DeploymentEnvironmentUpdated", "success") => "Configuration Applied".to_string(),
        ("DeploymentEnvironmentUpdated", "started") => {
            "Configuration Update In Progress".to_string()
        }
        ("DeploymentRedeployRequested", "failed") => "Redeployment Failed".to_string(),
        ("DeploymentRedeployRequested", "success") => "Deployment Redeployed".to_string(),
        ("DeploymentRedeployRequested", "started") => "Redeployment In Progress".to_string(),
        ("DeploymentCreated", _) => "Deployment Created".to_string(),
        ("DeploymentReleased", _)
            if data.get("previousReleaseId").is_some_and(Value::is_string) =>
        {
            "Release Updated".to_string()
        }
        ("DeploymentReleased", _) => "Initial Release".to_string(),
        ("DeploymentFailed", _) => "Deployment Failed".to_string(),
        ("DeploymentDegraded", _) => "Deployment Degraded".to_string(),
        ("DeploymentRecovered", _) => "Deployment Recovered".to_string(),
        ("DeploymentDeleted", _) => "Deployment Deleted".to_string(),
        ("ReleaseChannelUpdated", _) => "Release Channel Updated".to_string(),
        ("DeploymentReleaseChannelChanged", _) => "Release Channel Changed".to_string(),
        ("DeploymentRetryRequested", _) => "Deployment Retry Requested".to_string(),
        ("DeploymentRedeployRequested", _) => "Deployment Redeploy Requested".to_string(),
        ("DeploymentReleasePinned", _) => "Release Pinned".to_string(),
        ("DeploymentReleaseUnpinned", _) => "Release Unpinned".to_string(),
        ("DeploymentEnvironmentUpdated", _) => "Configuration Update Requested".to_string(),
        ("DeploymentDeletionRequested", _) => "Deployment Deletion Requested".to_string(),
        _ => humanize_event_type(event_type),
    }
}

fn event_details(data: &Value, state: &Value) -> String {
    let event_type = data.get("type").and_then(Value::as_str).unwrap_or_default();

    match event_type {
        "DeploymentEnvironmentUpdated" => data
            .get("changedKeys")
            .and_then(Value::as_array)
            .map(|keys| {
                let keys = keys.iter().filter_map(Value::as_str).collect::<Vec<_>>();
                if keys.is_empty() {
                    "Configuration updated".to_string()
                } else {
                    format!("Changed: {}", keys.join(", "))
                }
            })
            .unwrap_or_else(|| "Configuration updated".to_string()),
        "DeploymentReleased" => match (
            data.get("previousReleaseId").and_then(Value::as_str),
            data.get("releaseId").and_then(Value::as_str),
        ) {
            (Some(previous), Some(current)) => format!("{previous} → {current}"),
            (_, Some(current)) => format!("Initial release: {current}"),
            _ => String::new(),
        },
        "DeploymentFailed" => failure_details(data, state),
        "DeploymentDegraded" => failure_details(data, state),
        "DeploymentRecovered" => string_detail(data, "releaseId", "Now running"),
        "DeploymentRetryRequested" => string_detail(data, "attemptedReleaseId", "Retrying release"),
        "DeploymentRedeployRequested" => string_detail(data, "releaseId", "Release"),
        "DeploymentReleasePinned" => string_detail(data, "pinnedReleaseId", "Pinned to"),
        "DeploymentReleaseUnpinned" => {
            string_detail(data, "previousPinnedReleaseId", "Was pinned to")
        }
        "ReleaseChannelUpdated" => string_detail(data, "channel", "Channel"),
        "DeploymentReleaseChannelChanged" => match (
            data.get("previousChannel").and_then(Value::as_str),
            data.get("channel").and_then(Value::as_str),
        ) {
            (Some(previous), Some(current)) => format!("{previous} → {current}"),
            _ => String::new(),
        },
        "DeploymentDeleted" => "Resources torn down".to_string(),
        "DeploymentDeletionRequested" => "Deletion enqueued".to_string(),
        _ => generic_event_details(data),
    }
}

fn failure_details(data: &Value, state: &Value) -> String {
    let phase = data.get("phase").and_then(Value::as_str);
    let error = data.get("error").or_else(|| state.pointer("/failed/error"));
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str);

    [phase, code, message]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(": ")
}

fn generic_event_details(data: &Value) -> String {
    const DETAILS: &[(&str, &str)] = &[
        ("stack", "Stack"),
        ("image", "Image"),
        ("resourceName", "Resource"),
        ("platform", "Platform"),
        ("targetTriple", "Target"),
        ("stackName", "Stack"),
        ("cfnStackName", "Stack"),
        ("project", "Project"),
    ];

    DETAILS
        .iter()
        .find_map(|(field, label)| {
            data.get(*field)
                .and_then(Value::as_str)
                .map(|value| format!("{label}: {value}"))
        })
        .unwrap_or_default()
}

fn string_detail(data: &Value, field: &str, label: &str) -> String {
    data.get(field)
        .and_then(Value::as_str)
        .map(|value| format!("{label}: {value}"))
        .unwrap_or_default()
}

fn humanize_event_type(event_type: &str) -> String {
    let mut result = String::with_capacity(event_type.len() + 8);
    for (index, character) in event_type.chars().enumerate() {
        if index > 0 && character.is_uppercase() {
            result.push(' ');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_event_includes_user_and_changed_keys() {
        let row = EventDisplayRow::try_new(
            "event_1".to_string(),
            DateTime::parse_from_rfc3339("2026-08-20T06:26:10Z")
                .expect("timestamp should parse")
                .with_timezone(&Utc),
            &serde_json::json!({
                "type": "DeploymentEnvironmentUpdated",
                "deploymentId": "dep_1",
                "changedKeys": ["BETA", "ALPHA"],
                "actor": { "kind": "user", "id": "usr_1", "email": "dev@example.com" }
            }),
            &"success",
        )
        .expect("event should format");

        assert_eq!(row.state, "success");
        assert_eq!(row.actor, "dev@example.com");
        assert_eq!(row.event, "Configuration Applied");
        assert_eq!(row.details, "Changed: BETA, ALPHA");
    }

    #[test]
    fn asynchronous_event_without_actor_is_system_authored() {
        let row = EventDisplayRow::try_new(
            "event_2".to_string(),
            Utc::now(),
            &serde_json::json!({
                "type": "DeploymentReleased",
                "deploymentId": "dep_1",
                "previousReleaseId": "rel_old",
                "releaseId": "rel_new"
            }),
            &"success",
        )
        .expect("event should format");

        assert_eq!(row.actor, "System");
        assert_eq!(row.event, "Release Updated");
        assert_eq!(row.details, "rel_old → rel_new");
    }

    #[test]
    fn historical_user_intent_without_actor_is_not_labeled_as_system() {
        let row = EventDisplayRow::try_new(
            "event_legacy".to_string(),
            Utc::now(),
            &serde_json::json!({
                "type": "DeploymentReleasePinned",
                "deploymentId": "dep_1",
                "pinnedReleaseId": "rel_new"
            }),
            &"none",
        )
        .expect("event should format");

        assert_eq!(row.actor, "Unknown");
        assert_eq!(row.event, "Release Pinned");
    }

    #[test]
    fn failed_event_includes_phase_code_and_message() {
        let row = EventDisplayRow::try_new(
            "event_3".to_string(),
            Utc::now(),
            &serde_json::json!({
                "type": "DeploymentFailed",
                "phase": "updating",
                "error": { "code": "UPDATE_FAILED", "message": "candidate did not become ready" }
            }),
            &serde_json::json!({ "failed": { "error": null } }),
        )
        .expect("event should format");

        assert_eq!(row.state, "failed");
        assert_eq!(
            row.details,
            "updating: UPDATE_FAILED: candidate did not become ready"
        );
    }
}
