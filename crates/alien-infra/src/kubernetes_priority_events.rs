//! Why a StatefulSet or DaemonSet cannot create pods for a missing PriorityClass.
//!
//! Deployments report this as a `ReplicaFailure` condition. StatefulSets and DaemonSets only
//! emit a `FailedCreate` event, so the controllers read the namespace events instead.

use crate::core::ResourceControllerContext;
use alien_core::WorkloadPriority;
use k8s_openapi::api::core::v1::Event;

const MARKER: &str = "no PriorityClass with name ";

/// The failure text (with the values key to fix) when `workload_name` could not create pods
/// because the mapped PriorityClass does not exist. Only workloads that declare a priority are
/// checked. Reading events is best effort: any error yields `None`.
pub(crate) async fn missing_priority_class_progress(
    ctx: &ResourceControllerContext<'_>,
    priority: Option<WorkloadPriority>,
    namespace: &str,
    workload_name: &str,
) -> Option<String> {
    priority?;
    let config = ctx.get_kubernetes_config().ok()?;
    let client = ctx
        .service_provider
        .get_kubernetes_event_client(config)
        .await
        .ok()?;
    let events = client.list_events(namespace, None).await.ok()?;
    priority_failure_from_events(&events.items, workload_name)
}

pub(crate) fn priority_failure_from_events(
    events: &[Event],
    workload_name: &str,
) -> Option<String> {
    events
        .iter()
        .filter(|event| {
            event.reason.as_deref() == Some("FailedCreate")
                && event.involved_object.name.as_deref() == Some(workload_name)
                && event.message.as_deref().is_some_and(|m| m.contains(MARKER))
        })
        .max_by_key(|event| event.last_timestamp.as_ref().map(|t| t.0))
        .and_then(|event| event.message.as_deref())
        .map(|message| {
            alien_core::kubernetes_priority_class_failure_hint(&format!("FailedCreate: {message}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(reason: &str, object: &str, message: &str) -> Event {
        let mut event = Event::default();
        event.reason = Some(reason.to_string());
        event.involved_object.name = Some(object.to_string());
        event.message = Some(message.to_string());
        event
    }

    #[test]
    fn names_the_missing_class_for_the_matching_workload_only() {
        let message =
            "create Pod db-0 in StatefulSet db failed error: pods \"db-0\" is forbidden: no PriorityClass with name acme-critical was found";
        let events = vec![
            event("FailedCreate", "other", message),
            event("FailedCreate", "db", message),
        ];
        let hint = priority_failure_from_events(&events, "db").expect("hint");
        assert!(hint.contains("acme-critical"), "{hint}");
        assert!(hint.contains("workloadPriority"), "{hint}");
        assert_eq!(priority_failure_from_events(&events[..1], "db"), None);
        assert_eq!(
            priority_failure_from_events(&[event("FailedCreate", "db", "quota exceeded")], "db"),
            None
        );
    }
}
