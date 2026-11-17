//! Pub/Sub dead-letter policies. Shared by the Queue controller (the pull subscription) and the
//! Worker controller (the push subscription of a queue trigger).

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use alien_core::Queue;
use alien_error::{AlienError, Context};
use alien_gcp_clients::{
    iam::{Binding, IamPolicy},
    pubsub::DeadLetterPolicy,
};

pub(crate) const PUBLISHER: &str = "roles/pubsub.publisher";
pub(crate) const SUBSCRIBER: &str = "roles/pubsub.subscriber";

/// The dead-letter policy for a subscription of `queue`'s topic, when it has a dead-letter
/// setting. The target topic follows from its deterministic name.
pub(crate) fn dead_letter_policy(
    ctx: &ResourceControllerContext<'_>,
    queue: &Queue,
) -> Result<Option<DeadLetterPolicy>> {
    let Some(dead_letter) = &queue.dead_letter else {
        return Ok(None);
    };
    let project = &ctx.get_gcp_config()?.project_id;
    let topic = format!("{}-{}", ctx.resource_prefix, dead_letter.queue.id);
    Ok(Some(DeadLetterPolicy {
        dead_letter_topic: Some(format!("projects/{project}/topics/{topic}")),
        max_delivery_attempts: Some(dead_letter.max_attempts as i32),
    }))
}

/// A comparable form of a policy, kept in controller state to notice a change.
pub(crate) fn policy_key(policy: &Option<DeadLetterPolicy>) -> Option<String> {
    policy.as_ref().map(|policy| {
        format!(
            "{}:{}",
            policy.dead_letter_topic.as_deref().unwrap_or_default(),
            policy.max_delivery_attempts.unwrap_or_default()
        )
    })
}

/// The Pub/Sub service agent, which moves messages: `roles/pubsub.publisher` on the target topic
/// and `roles/pubsub.subscriber` on each source subscription are all it needs.
pub(crate) async fn pubsub_service_agent(
    ctx: &ResourceControllerContext<'_>,
    resource_id: &str,
) -> Result<String> {
    let cfg = ctx.get_gcp_config()?;
    let number = match cfg
        .project_number
        .as_deref()
        .filter(|number| !number.is_empty())
    {
        Some(number) => number.to_string(),
        None => ctx
            .service_provider
            .get_gcp_resource_manager_client(cfg)?
            .get_project_metadata(cfg.project_id.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                resource_id: Some(resource_id.to_string()),
                message: "Read the project number for the Pub/Sub service agent".into(),
            })?
            .project_number
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    resource_id: Some(resource_id.to_string()),
                    message: "The project has no project number".into(),
                })
            })?,
    };
    Ok(format!(
        "serviceAccount:service-{number}@gcp-sa-pubsub.iam.gserviceaccount.com"
    ))
}

/// Adds or removes `member` from `role`, preserving unrelated and conditional grants and the
/// optimistic-concurrency etag. Returns whether the policy changed.
pub(crate) fn set_role_member(
    policy: &mut IamPolicy,
    role: &str,
    member: &str,
    enabled: bool,
) -> bool {
    if enabled {
        if let Some(binding) = policy
            .bindings
            .iter_mut()
            .find(|binding| binding.role == role && binding.condition.is_none())
        {
            if binding.members.iter().any(|existing| existing == member) {
                return false;
            }
            binding.members.push(member.to_string());
        } else {
            policy.bindings.push(Binding {
                role: role.into(),
                members: vec![member.into()],
                condition: None,
            });
        }
        true
    } else {
        let mut changed = false;
        for binding in &mut policy.bindings {
            if binding.role == role && binding.condition.is_none() {
                binding.members.retain(|existing| {
                    if existing == member {
                        changed = true;
                        false
                    } else {
                        true
                    }
                });
            }
        }
        if changed {
            policy
                .bindings
                .retain(|binding| !binding.members.is_empty());
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(bindings: &[(&str, &[&str])]) -> IamPolicy {
        IamPolicy {
            bindings: bindings
                .iter()
                .map(|(role, members)| Binding {
                    role: (*role).into(),
                    members: members.iter().map(|m| (*m).to_string()).collect(),
                    condition: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_grant_is_added_once_and_removed_without_touching_other_members() {
        let agent = "serviceAccount:service-1@gcp-sa-pubsub.iam.gserviceaccount.com";
        let mut p = policy(&[(PUBLISHER, &["serviceAccount:other@x"])]);
        assert!(set_role_member(&mut p, PUBLISHER, agent, true));
        assert!(!set_role_member(&mut p, PUBLISHER, agent, true));
        assert_eq!(p.bindings[0].members.len(), 2);
        assert!(set_role_member(&mut p, PUBLISHER, agent, false));
        assert_eq!(p.bindings[0].members, vec!["serviceAccount:other@x"]);
        assert!(!set_role_member(&mut p, PUBLISHER, agent, false));
    }

    #[test]
    fn removing_the_last_member_removes_the_binding() {
        let agent = "serviceAccount:a@x";
        let mut p = policy(&[(SUBSCRIBER, &[agent])]);
        assert!(set_role_member(&mut p, SUBSCRIBER, agent, false));
        assert!(p.bindings.is_empty());
    }
}
