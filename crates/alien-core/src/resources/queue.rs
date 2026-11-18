use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Represents a message queue resource with minimal, portable semantics.
/// Queue integrates with platform-native services (AWS SQS, GCP Pub/Sub, Azure Service Bus).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Queue {
    /// Identifier for the queue. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]).
    /// Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// Move a message to another Queue in the stack after it failed to be processed
    /// `max_attempts` times. Omitted when absent so existing serialized configurations and
    /// setup digests do not change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_letter: Option<QueueDeadLetter>,
}

/// Default number of delivery attempts before a message moves to the dead-letter queue.
pub const DEFAULT_DEAD_LETTER_MAX_ATTEMPTS: u32 = 5;
/// Fewest delivery attempts a dead-letter setting accepts (the Pub/Sub range).
pub const MIN_DEAD_LETTER_MAX_ATTEMPTS: u32 = 5;
/// Most delivery attempts a dead-letter setting accepts (the Pub/Sub range).
pub const MAX_DEAD_LETTER_MAX_ATTEMPTS: u32 = 100;

fn default_dead_letter_max_attempts() -> u32 {
    DEFAULT_DEAD_LETTER_MAX_ATTEMPTS
}

/// Where a Queue moves messages that keep failing.
///
/// The target is a normal Queue in the same stack with the same lifecycle. The attempt count is
/// approximate: it is the cloud's own delivery count (SQS counts every receive, Pub/Sub is
/// best-effort). The payload is preserved; there is no portable failure reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueDeadLetter {
    /// The Queue that receives the messages. Must be a Queue in the stack with the same
    /// lifecycle, must not be this Queue and must not have a dead-letter setting itself.
    pub queue: ResourceRef,

    /// Deliveries after which the message moves (5 to 100, default 5).
    #[serde(default = "default_dead_letter_max_attempts")]
    #[builder(default = default_dead_letter_max_attempts())]
    #[cfg_attr(feature = "openapi", schema(minimum = 5, maximum = 100, default = 5))]
    pub max_attempts: u32,
}

impl Queue {
    /// The resource type identifier for Queue
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("queue");

    /// Returns the queue's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// SQS keeps messages for 4 days by default. A dead-lettered message keeps its original send
/// time, so a queue that receives them keeps messages for the SQS maximum of 14 days.
pub const AWS_DEFAULT_RETENTION_SECONDS: u32 = 4 * 24 * 60 * 60;
/// Retention for an SQS queue that is a dead-letter target.
pub const AWS_DEAD_LETTER_TARGET_RETENTION_SECONDS: u32 = 14 * 24 * 60 * 60;

/// Ids of the Queues in `stack` that dead-letter into `queue_id`, sorted.
pub fn queues_dead_lettering_to(stack: &crate::Stack, queue_id: &str) -> Vec<String> {
    let mut sources: Vec<String> = stack
        .resources
        .iter()
        .filter(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Queue>()
                .and_then(|queue| queue.dead_letter.as_ref())
                .is_some_and(|dead_letter| dead_letter.queue.id == queue_id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    sources.sort();
    sources
}

/// Whether the Queue `queue_id` of `stack` has a dead-letter setting.
pub fn queue_has_dead_letter(stack: &crate::Stack, queue_id: &str) -> bool {
    stack
        .resources
        .get(queue_id)
        .and_then(|entry| entry.config.downcast_ref::<Queue>())
        .is_some_and(|queue| queue.dead_letter.is_some())
}

/// Whether a Live Worker of `stack` is triggered by the Queue `queue_id`. (A setup-owned Worker's
/// subscription is written by setup, which also grants on it directly.)
pub fn queue_has_worker_trigger(stack: &crate::Stack, queue_id: &str) -> bool {
    stack.resources.values().any(|entry| {
        entry.lifecycle == crate::ResourceLifecycle::Live
            && entry
            .config
            .downcast_ref::<crate::Worker>()
            .is_some_and(|worker| {
                worker.triggers.iter().any(|trigger| {
                    matches!(trigger, crate::WorkerTrigger::Queue { queue, .. } if queue.id == queue_id)
                })
            })
    })
}

/// Whether the Queue `queue_id` is setup-owned (Frozen), has a dead-letter setting and is the
/// trigger of a Worker. A Worker's push subscription on such a queue is not written by anything
/// that holds IAM write access at runtime, so setup grants the Pub/Sub service agent
/// `roles/pubsub.subscriber` for the project once (Google's documented grant for dead-letter
/// subscriptions) instead. See `queues_needing_project_push_subscriber`.
pub fn queue_needs_project_push_subscriber(stack: &crate::Stack, queue_id: &str) -> bool {
    stack.resources.get(queue_id).is_some_and(|entry| {
        entry.lifecycle == crate::ResourceLifecycle::Frozen
            && entry
                .config
                .downcast_ref::<Queue>()
                .is_some_and(|queue| queue.dead_letter.is_some())
    }) && queue_has_worker_trigger(stack, queue_id)
}

/// The Queues of `stack` that need the project-level subscriber grant of the Pub/Sub service
/// agent, sorted. One grant serves all of them, so it stays while any of them remains.
pub fn queues_needing_project_push_subscriber(stack: &crate::Stack) -> Vec<String> {
    let mut ids: Vec<String> = stack
        .resources
        .keys()
        .filter(|id| queue_needs_project_push_subscriber(stack, id))
        .cloned()
        .collect();
    ids.sort();
    ids
}

/// Outputs generated by a successfully provisioned Queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct QueueOutputs {
    /// The name of the queue (platform-specific physical name).
    pub queue_name: String,
    /// Platform-specific identifier or URL (e.g., SQS URL, Pub/Sub topic/subscription).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
}

impl ResourceOutputsDefinition for QueueOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Queue::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<QueueOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

// Implementation of ResourceDefinition trait for Queue
impl ResourceDefinition for Queue {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn replace_after_failed_create_is_safe(&self) -> bool {
        // Deleting it destroys messages not yet consumed.
        false
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        self.dead_letter
            .iter()
            .map(|dead_letter| dead_letter.queue.clone())
            .collect()
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        // Downcast to Queue type to use the existing validate_update method
        let new_queue = new_config.as_any().downcast_ref::<Queue>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: Self::RESOURCE_TYPE,
                actual: new_config.get_resource_type(),
            })
        })?;

        if self.id != new_queue.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Queue>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_builder() {
        let q = Queue::new("my-queue".to_string()).build();
        assert_eq!(q.id, "my-queue");
    }

    #[test]
    fn test_queue_resource_type() {
        assert_eq!(Queue::RESOURCE_TYPE.as_ref(), "queue");
    }

    #[test]
    fn test_queue_resource_definition() {
        let q = Queue::new("test-q".to_string()).build();
        assert_eq!(q.get_resource_type(), Queue::RESOURCE_TYPE);
        assert_eq!(q.id(), "test-q");
        assert!(q.get_dependencies().is_empty());
    }

    #[test]
    fn absent_dead_letter_serializes_exactly_as_before() {
        let q = Queue::new("tasks".to_string()).build();
        assert_eq!(serde_json::to_string(&q).unwrap(), r#"{"id":"tasks"}"#);
        let parsed: Queue = serde_json::from_str(r#"{"id":"tasks"}"#).unwrap();
        assert_eq!(parsed, q);
    }

    #[test]
    fn dead_letter_round_trips_in_camel_case_and_is_a_dependency() {
        let target = ResourceRef::new(Queue::RESOURCE_TYPE, "failed");
        let q = Queue::new("tasks".to_string())
            .dead_letter(
                QueueDeadLetter::builder()
                    .queue(target.clone())
                    .max_attempts(7)
                    .build(),
            )
            .build();
        let json = serde_json::to_value(&q).unwrap();
        assert_eq!(json["deadLetter"]["maxAttempts"], 7);
        assert_eq!(json["deadLetter"]["queue"]["id"], "failed");
        assert_eq!(serde_json::from_value::<Queue>(json).unwrap(), q);
        assert_eq!(q.get_dependencies(), vec![target]);
    }

    #[test]
    fn dead_letter_max_attempts_defaults_to_five_and_rejects_unknown_fields() {
        let parsed: Queue = serde_json::from_value(serde_json::json!({
            "id": "tasks",
            "deadLetter": {"queue": {"type": "queue", "id": "failed"}}
        }))
        .unwrap();
        assert_eq!(parsed.dead_letter.unwrap().max_attempts, 5);
        assert!(serde_json::from_value::<Queue>(serde_json::json!({
            "id": "tasks",
            "deadLetter": {"queue": {"type": "queue", "id": "failed"}, "retries": 3}
        }))
        .is_err());
    }

    #[test]
    fn dead_letter_can_be_added_changed_and_removed_by_update() {
        let plain = Queue::new("tasks".to_string()).build();
        let with = Queue::new("tasks".to_string())
            .dead_letter(
                QueueDeadLetter::builder()
                    .queue(ResourceRef::new(Queue::RESOURCE_TYPE, "failed"))
                    .build(),
            )
            .build();
        assert!(plain.validate_update(&with).is_ok());
        assert!(with.validate_update(&plain).is_ok());
    }

    #[test]
    fn test_queue_validate_update() {
        let original = Queue::new("test-q".to_string()).build();
        let valid_update = Queue::new("test-q".to_string()).build();
        let invalid_update = Queue::new("different-q".to_string()).build();

        assert!(original.validate_update(&valid_update).is_ok());
        assert!(original.validate_update(&invalid_update).is_err());
    }
}
