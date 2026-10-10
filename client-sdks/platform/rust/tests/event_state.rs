//! `alien managers events` and `alien deployments events` decode every event through these
//! generated types. The Platform API stores each event's state as an `alien_core::EventState`,
//! so every variant must survive the generated types, or the whole page fails to decode.

use alien_core::EventState;
use alien_platform_api::types::{Event, EventListItemResponse};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

fn states() -> Vec<EventState> {
    vec![
        EventState::None,
        EventState::Started,
        EventState::Success,
        EventState::Failed { error: None },
        EventState::Failed {
            error: Some(
                serde_json::from_value(json!({
                    "code": "UPDATE_FAILED",
                    "message": "candidate did not become ready",
                    "internal": false,
                    "retryable": true
                }))
                .expect("error decodes"),
            ),
        },
    ]
}

// An event as the Platform API returns it, with the given state.
fn event(state: &EventState) -> Value {
    json!({
        "id": "event_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "deploymentId": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "releaseId": "rel_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "debugSessionId": null,
        "data": {
            "type": "DeploymentReleased",
            "deploymentId": "dep_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "previousReleaseId": null,
            "releaseId": "rel_aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        "state": serde_json::to_value(state).expect("state encodes"),
        "projectId": "prj_aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "createdAt": "2026-10-10T06:26:48.506Z",
        "workspaceId": "ws_AAAAAAAAAAAAAAAAAAAAAAAA"
    })
}

fn round_trip<T: DeserializeOwned + Serialize>(state: &EventState) -> EventState {
    let decoded: T = serde_json::from_value(event(state))
        .unwrap_or_else(|error| panic!("event with state {state:?} decodes: {error}"));
    let encoded = serde_json::to_value(&decoded).expect("event encodes");
    serde_json::from_value(encoded["state"].clone()).expect("state decodes")
}

#[test]
fn manager_events_keep_every_event_state() {
    for state in states() {
        assert_eq!(round_trip::<Event>(&state), state);
    }
}

#[test]
fn deployment_events_keep_every_event_state() {
    for state in states() {
        assert_eq!(round_trip::<EventListItemResponse>(&state), state);
    }
}
