//! Deadline enforcement owned by the controller, independent of workload requests.

use alien_azure_clients::azure::sandbox_data_plane::SandboxDataPlaneApi;
use alien_core::SANDBOX_EXPIRY_LABEL;
use alien_error::Context;

use crate::error::{ErrorData, Result};

/// Starts deleting every expired sandbox in the group. The next controller tick observes
/// in-flight deletions; repeated delete requests are safe. A declaration bounds even a caller
/// that omits or widens its per-session deadline label.
pub async fn reap_expired_azure_sandboxes(
    client: &dyn SandboxDataPlaneApi,
    group: &str,
    max_lifetime_seconds: Option<u32>,
) -> Result<Vec<String>> {
    let context = || ErrorData::CloudPlatformError {
        message: "Could not enforce Azure sandbox lifetimes".to_string(),
        resource_id: Some(group.to_string()),
    };
    let sessions = client.list_sandboxes(group).await.context(context())?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut deleted = Vec::new();
    let mut first_error = None;
    for session in sessions {
        if session.state.eq_ignore_ascii_case("Deleting") {
            continue;
        }
        let declared = max_lifetime_seconds
            .map(|seconds| session.created_at.timestamp_millis() + i64::from(seconds) * 1000);
        let requested = session.labels.get(SANDBOX_EXPIRY_LABEL).map(|value| {
            value.parse::<i64>().unwrap_or_else(|_| {
                tracing::warn!(sandbox_id = %session.id, "invalid expiry label; deleting the session");
                i64::MIN
            })
        });
        let expires = match (declared, requested) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if expires.is_some_and(|expires| expires <= now) {
            match client.delete_sandbox(group, &session.id).await {
                Ok(()) => deleted.push(session.id),
                Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error).context(context());
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_azure_clients::azure::sandbox_data_plane::{ListedSandbox, MockSandboxDataPlaneApi};
    use alien_error::AlienError;
    use std::collections::BTreeMap;

    fn session(id: &str, age: i64, requested: Option<i64>) -> ListedSandbox {
        let now = chrono::Utc::now();
        ListedSandbox {
            id: id.to_string(),
            created_at: now - chrono::Duration::seconds(age),
            state: "Running".to_string(),
            labels: requested
                .map(|delta| {
                    BTreeMap::from([(
                        SANDBOX_EXPIRY_LABEL.to_string(),
                        (now.timestamp_millis() + delta * 1000).to_string(),
                    )])
                })
                .unwrap_or_default(),
        }
    }

    #[tokio::test]
    async fn declaration_and_request_choose_the_earlier_deadline() {
        let mut client = MockSandboxDataPlaneApi::new();
        client.expect_list_sandboxes().returning(|_| {
            Ok(vec![
                session("omitted", 60, None),
                session("widened", 60, Some(3600)),
                session("shortened", 0, Some(-1)),
                session("future", 0, Some(3600)),
                ListedSandbox {
                    state: "Deleting".to_string(),
                    ..session("already-deleting", 60, None)
                },
            ])
        });
        for id in ["omitted", "widened", "shortened"] {
            client
                .expect_delete_sandbox()
                .withf(move |_, found| found == id)
                .times(1)
                .returning(|_, _| Ok(()));
        }
        let deleted = reap_expired_azure_sandboxes(&client, "group", Some(30))
            .await
            .expect("reap");
        assert_eq!(deleted, ["omitted", "widened", "shortened"]);
    }

    #[tokio::test]
    async fn one_failed_delete_does_not_starve_other_expired_sessions() {
        let mut client = MockSandboxDataPlaneApi::new();
        client
            .expect_list_sandboxes()
            .returning(|_| Ok(vec![session("busy", 60, None), session("other", 60, None)]));
        client
            .expect_delete_sandbox()
            .withf(|_, id| id == "busy")
            .times(1)
            .returning(|_, _| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RemoteResourceConflict {
                        resource_type: "sandbox".to_string(),
                        resource_name: "busy".to_string(),
                        message: "retry".to_string(),
                    },
                ))
            });
        client
            .expect_delete_sandbox()
            .withf(|_, id| id == "other")
            .times(1)
            .returning(|_, _| Ok(()));
        reap_expired_azure_sandboxes(&client, "group", Some(30))
            .await
            .expect_err("failure remains visible for retry");
    }
}
