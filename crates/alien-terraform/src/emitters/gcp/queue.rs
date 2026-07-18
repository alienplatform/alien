//! GCP Queue — Pub/Sub topic plus default subscription.
//!
//! Each Alien Queue maps to one topic + one default pull subscription.
//! Functions triggered by the queue land additional push subscriptions
//! at function-emit time; this file owns only the topic + the
//! controller-readable subscription.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::gates::{permission_gate_count, TrackedPermissionRef},
    emitters::gcp::helpers::{
        binding_label_for_role, downcast, emit_custom_roles_for_bindings, labels,
        permission_context, required_label, resource_prefix_template, role_expression_for_binding,
        service_account_member_for_label,
    },
    expr,
};
use alien_core::{import::EmitContext, ErrorData, Queue, Result, Worker, WorkerTrigger};
use alien_error::AlienError;
use alien_permissions::{
    generators::{GcpBindingResourceKind, GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    BindingTarget,
};
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpQueueEmitter;

impl TfEmitter for GcpQueueEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let queue = downcast::<Queue>(ctx, Queue::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let topic = resource_block(
            "google_pubsub_topic",
            label,
            [
                attr("name", resource_prefix_template(queue.id())),
                attr("project", expr::raw("var.gcp_project")),
                attr("labels", labels(ctx, "queue")),
            ],
        );

        let ack_deadline = ack_deadline_for(ctx);
        let subscription = resource_block(
            "google_pubsub_subscription",
            label,
            [
                attr(
                    "name",
                    expr::template(format!("${{local.resource_prefix}}-{}-default", queue.id())),
                ),
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "topic",
                    expr::traversal(["google_pubsub_topic", label, "id"]),
                ),
                attr(
                    "ack_deadline_seconds",
                    Expression::Number(hcl::Number::from(i64::from(ack_deadline))),
                ),
                attr(
                    "message_retention_duration",
                    Expression::String("604800s".to_string()),
                ),
                attr("enable_message_ordering", Expression::Bool(false)),
                nested(block(
                    "expiration_policy",
                    [attr("ttl", Expression::String("".to_string()))],
                )),
                attr("labels", labels(ctx, "queue")),
            ],
        );

        let mut fragment = TfFragment::default();
        fragment.resource_blocks.push(topic);
        fragment.resource_blocks.push(subscription);
        emit_queue_iam(ctx, &mut fragment, label)?;
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "topicId",
                expr::traversal(["google_pubsub_topic", label, "name"]),
            ),
            (
                "topicName",
                expr::traversal(["google_pubsub_topic", label, "id"]),
            ),
            (
                "subscriptionId",
                expr::traversal(["google_pubsub_subscription", label, "name"]),
            ),
            (
                "subscriptionName",
                expr::traversal(["google_pubsub_subscription", label, "id"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("pubsub".to_string())),
            (
                "topic",
                expr::traversal(["google_pubsub_topic", label, "id"]),
            ),
            (
                "subscription",
                expr::traversal(["google_pubsub_subscription", label, "id"]),
            ),
        ])))
    }
}

fn emit_queue_iam(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str) -> Result<()> {
    for owner in queue_permission_owners(ctx) {
        let member = service_account_member_for_label(&owner.label);
        let context = permission_context(&owner.label, ctx.stack.id())
            .with_resource_name(format!("${{google_pubsub_topic.{label}.name}}"));
        let generator = GcpRuntimePermissionsGenerator::new();

        for tracked_ref in owner.refs {
            let Some(permission_set) = tracked_ref
                .reference
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with("queue/") {
                continue;
            }

            let gate_count = match &owner.profile {
                Some(profile) => permission_gate_count(
                    ctx,
                    profile,
                    &permission_set.id,
                    &tracked_ref.origin_keys(ctx.resource_id),
                )?,
                None => None,
            };
            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Resource, &context)
                .map_err(|err| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!(
                            "failed to generate GCP queue IAM grant plan for '{}': {}",
                            permission_set.id, err
                        ),
                    })
                })?;
            let bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
            let custom_roles = emit_custom_roles_for_bindings(fragment, &grant_plan, &bindings)?;

            for (idx, binding) in bindings.into_iter().enumerate() {
                let Some(resource_kind) = binding.resource_kind else {
                    continue;
                };
                let role_label = binding_label_for_role(&binding.role, &custom_roles)?;
                let role = role_expression_for_binding(&binding.role, &custom_roles)?;
                let mut body = Vec::new();
                if let Some(gate_count) = &gate_count {
                    body.push(attr("count", gate_count.clone()));
                }
                match resource_kind {
                    GcpBindingResourceKind::PubsubTopic => {
                        body.extend([
                            attr("project", expr::raw("var.gcp_project")),
                            attr(
                                "topic",
                                expr::traversal(["google_pubsub_topic", label, "name"]),
                            ),
                            attr("role", role),
                            attr("member", member.clone()),
                        ]);
                        fragment.resource_blocks.push(resource_block(
                            "google_pubsub_topic_iam_member",
                            &format!("{role_label}_{label}_{}_topic_{idx}", owner.label),
                            body,
                        ));
                    }
                    GcpBindingResourceKind::PubsubSubscription => {
                        body.extend([
                            attr("project", expr::raw("var.gcp_project")),
                            attr(
                                "subscription",
                                expr::traversal(["google_pubsub_subscription", label, "name"]),
                            ),
                            attr("role", role),
                            attr("member", member.clone()),
                        ]);
                        fragment.resource_blocks.push(resource_block(
                            "google_pubsub_subscription_iam_member",
                            &format!("{role_label}_{label}_{}_subscription_{idx}", owner.label),
                            body,
                        ));
                    }
                    GcpBindingResourceKind::ArtifactRegistryRepository => {}
                }
            }
        }
    }

    Ok(())
}

struct QueueOwner {
    label: String,
    profile: Option<String>,
    refs: Vec<TrackedPermissionRef>,
}

fn queue_permission_owners(ctx: &EmitContext<'_>) -> Vec<QueueOwner> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = queue_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = ctx.name_for(&service_account_id) {
            owners.push(QueueOwner {
                label: label.to_string(),
                profile: Some(profile_name.clone()),
                refs,
            });
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = queue_permission_refs(profile, ctx.resource_id);
        if !refs.is_empty() {
            if let Some((management_id, _entry)) = ctx.stack.resources().find(|(_id, entry)| {
                entry.config.resource_type() == alien_core::RemoteStackManagement::RESOURCE_TYPE
            }) {
                if let Some(label) = ctx.name_for(management_id) {
                    owners.push(QueueOwner {
                        label: label.to_string(),
                        profile: None,
                        refs,
                    });
                }
            }
        }
    }

    owners
}

fn queue_permission_refs(
    profile: &alien_core::PermissionProfile,
    resource_id: &str,
) -> Vec<TrackedPermissionRef> {
    let mut refs: Vec<TrackedPermissionRef> = Vec::new();
    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if !refs
                .iter()
                .any(|tracked| tracked.reference.id() == permission_ref.id())
            {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: true,
                    in_wildcard: false,
                });
            }
        }
    }
    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("queue/"))
        {
            if let Some(tracked) = refs
                .iter_mut()
                .find(|tracked| tracked.reference.id() == permission_ref.id())
            {
                tracked.in_wildcard = true;
            } else {
                refs.push(TrackedPermissionRef {
                    reference: permission_ref.clone(),
                    in_resource: false,
                    in_wildcard: true,
                });
            }
        }
    }
    refs
}

fn ack_deadline_for(ctx: &EmitContext<'_>) -> u32 {
    let mut max_function_timeout = 0u32;
    for (_id, entry) in ctx.stack.resources() {
        let Some(function) = entry.config.downcast_ref::<Worker>() else {
            continue;
        };
        if function.triggers.iter().any(|trigger| {
            matches!(
                trigger,
                WorkerTrigger::Queue { queue }
                    if queue.resource_type == Queue::RESOURCE_TYPE && queue.id == ctx.resource_id
            )
        }) {
            max_function_timeout = max_function_timeout.max(function.timeout_seconds);
        }
    }
    if max_function_timeout == 0 {
        return 60;
    }
    // Pub/Sub max ack-deadline is 600 seconds.
    max_function_timeout.saturating_mul(2).clamp(10, 600)
}
