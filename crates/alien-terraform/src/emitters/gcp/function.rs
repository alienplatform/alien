//! GCP Function — Cloud Run service plus event-source bridges.
//!
//! Image-only (no source-build wiring); the source-build path lives in
//! `alien-build`'s pipeline. Triggers translate to:
//!
//! * `Queue` → `google_pubsub_subscription` push subscription pointing
//!   at the Cloud Run URL plus an OIDC token sourced from the function
//!   service account.
//! * `Schedule` → `google_cloud_scheduler_job` HTTP target.
//! * `Storage` → `google_eventarc_trigger` for storage events
//!   (`google.cloud.storage.object.v1.<event>`).
//!
//! Public ingress maps to `INGRESS_TRAFFIC_ALL`; private maps to
//! `INGRESS_TRAFFIC_INTERNAL_ONLY`.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::function_environment::{function_environment, GcpFunctionEnvironmentRenderer},
    emitters::gcp::helpers::{
        downcast, label_for_ref, labels, required_label, service_account_email,
    },
    expr,
    registry::TfRegistry,
};
use alien_core::{
    crontab_to_eventbridge::crontab_to_eventbridge, import::EmitContext, ErrorData, Function,
    FunctionCode, FunctionTrigger, Ingress, Result,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpFunctionEmitter;

impl TfEmitter for GcpFunctionEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let registry = TfRegistry::built_in();
        self.emit_with_registry(ctx, &registry)
    }

    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &TfRegistry,
    ) -> Result<TfFragment> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let FunctionCode::Image { image } = &function.code else {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_terraform_module".to_string(),
                reason: format!(
                    "function '{}' uses source code; Terraform modules require a pre-built image",
                    function.id
                ),
            }));
        };
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        let service_account = service_account_email(ctx, &function.permissions);

        let ingress = match function.ingress {
            Ingress::Public => "INGRESS_TRAFFIC_ALL",
            Ingress::Private => "INGRESS_TRAFFIC_INTERNAL_ONLY",
        };

        let container_attrs: Vec<hcl::structure::Structure> = vec![
            attr("image", Expression::String(image.clone())),
            nested(block(
                "resources",
                [
                    attr(
                        "limits",
                        expr::object([
                            ("cpu", Expression::String("1".to_string())),
                            (
                                "memory",
                                Expression::String(format!("{}Mi", function.memory_mb.max(128))),
                            ),
                        ]),
                    ),
                    attr("cpu_idle", Expression::Bool(true)),
                ],
            )),
        ];

        let mut env_blocks: Vec<hcl::structure::Structure> = Vec::new();
        let env_renderer = GcpFunctionEnvironmentRenderer {
            ctx,
            registry,
            function_id: &function.id,
        };
        for (k, v) in function_environment(function, alien_core::Platform::Gcp, &env_renderer)? {
            env_blocks.push(nested(block(
                "env",
                [attr("name", Expression::String(k)), attr("value", v)],
            )));
        }

        let mut container_body = container_attrs;
        container_body.extend(env_blocks);

        let mut template_body: Vec<hcl::structure::Structure> = vec![
            attr(
                "timeout",
                Expression::String(format!("{}s", function.timeout_seconds.max(1))),
            ),
            nested(block("containers", container_body)),
        ];
        if let Some(sa) = &service_account {
            template_body.push(attr("service_account", sa.clone()));
        }

        let mut service_body: Vec<hcl::structure::Structure> = vec![
            attr(
                "name",
                expr::template(format!("${{var.stack_name}}-{}", function.id)),
            ),
            attr("project", expr::raw("var.gcp_project")),
            attr("location", expr::raw("var.gcp_region")),
            attr("ingress", Expression::String(ingress.to_string())),
            attr("labels", labels(ctx, "function")),
            nested(block("template", template_body)),
        ];
        if matches!(function.ingress, Ingress::Public) {
            service_body.push(attr("invoker_iam_disabled", Expression::Bool(true)));
        }

        fragment.resource_blocks.push(resource_block(
            "google_cloud_run_v2_service",
            label,
            service_body,
        ));

        for (index, trigger) in function.triggers.iter().enumerate() {
            match trigger {
                FunctionTrigger::Queue { queue } => {
                    let queue_label = label_for_ref(ctx, queue)?;
                    let sub_label = format!("{label}_queue_{index}");
                    let mut push_body: Vec<hcl::structure::Structure> = vec![attr(
                        "push_endpoint",
                        expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
                    )];
                    if let Some(sa) = &service_account {
                        push_body.push(nested(block(
                            "oidc_token",
                            [
                                attr("service_account_email", sa.clone()),
                                attr(
                                    "audience",
                                    expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
                                ),
                            ],
                        )));
                    }
                    fragment.resource_blocks.push(resource_block(
                        "google_pubsub_subscription",
                        &sub_label,
                        [
                            attr(
                                "name",
                                expr::template(format!(
                                    "${{var.stack_name}}-{}-from-{}",
                                    function.id, queue.id
                                )),
                            ),
                            attr("project", expr::raw("var.gcp_project")),
                            attr(
                                "topic",
                                expr::traversal(["google_pubsub_topic", queue_label, "id"]),
                            ),
                            attr(
                                "ack_deadline_seconds",
                                Expression::Number(hcl::Number::from(i64::from(
                                    function.timeout_seconds.saturating_mul(2).clamp(10, 600),
                                ))),
                            ),
                            nested(block("push_config", push_body)),
                        ],
                    ));
                }
                FunctionTrigger::Schedule { cron } => {
                    let job_label = format!("{label}_schedule_{index}");
                    let normalized_cron = match crontab_to_eventbridge(cron) {
                        Ok(_) => cron.clone(),
                        Err(_) => cron.clone(),
                    };
                    let mut http_target_body: Vec<hcl::structure::Structure> = vec![
                        attr(
                            "uri",
                            expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
                        ),
                        attr("http_method", Expression::String("POST".to_string())),
                    ];
                    if let Some(sa) = &service_account {
                        http_target_body.push(nested(block(
                            "oidc_token",
                            [
                                attr("service_account_email", sa.clone()),
                                attr(
                                    "audience",
                                    expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
                                ),
                            ],
                        )));
                    }
                    fragment.resource_blocks.push(resource_block(
                        "google_cloud_scheduler_job",
                        &job_label,
                        [
                            attr(
                                "name",
                                expr::template(format!(
                                    "${{var.stack_name}}-{}-sched-{}",
                                    function.id, index
                                )),
                            ),
                            attr("project", expr::raw("var.gcp_project")),
                            attr("region", expr::raw("var.gcp_region")),
                            attr("schedule", Expression::String(normalized_cron)),
                            attr("time_zone", Expression::String("Etc/UTC".to_string())),
                            nested(block("http_target", http_target_body)),
                        ],
                    ));
                }
                FunctionTrigger::Storage { storage, events } => {
                    let storage_label = label_for_ref(ctx, storage)?;
                    let trig_label = format!("{label}_storage_{index}");
                    let event_type = if events.iter().any(|e| e == "deleted") {
                        "google.cloud.storage.object.v1.deleted"
                    } else {
                        "google.cloud.storage.object.v1.finalized"
                    };
                    let mut destination_body: Vec<hcl::structure::Structure> = vec![nested(block(
                        "cloud_run_service",
                        [
                            attr(
                                "service",
                                expr::traversal(["google_cloud_run_v2_service", label, "name"]),
                            ),
                            attr(
                                "region",
                                expr::traversal(["google_cloud_run_v2_service", label, "location"]),
                            ),
                        ],
                    ))];
                    let _ = &mut destination_body;
                    let mut trigger_body: Vec<hcl::structure::Structure> = vec![
                        attr(
                            "name",
                            expr::template(format!(
                                "${{var.stack_name}}-{}-{}-storage",
                                function.id, storage.id
                            )),
                        ),
                        attr("project", expr::raw("var.gcp_project")),
                        attr("location", expr::raw("var.gcp_region")),
                        nested(block(
                            "matching_criteria",
                            [
                                attr("attribute", Expression::String("type".to_string())),
                                attr("value", Expression::String(event_type.to_string())),
                            ],
                        )),
                        nested(block(
                            "matching_criteria",
                            [
                                attr("attribute", Expression::String("bucket".to_string())),
                                attr(
                                    "value",
                                    expr::traversal([
                                        "google_storage_bucket",
                                        storage_label,
                                        "name",
                                    ]),
                                ),
                            ],
                        )),
                        nested(block("destination", destination_body)),
                    ];
                    if let Some(sa) = &service_account {
                        trigger_body.push(attr("service_account", sa.clone()));
                    }
                    fragment.resource_blocks.push(resource_block(
                        "google_eventarc_trigger",
                        &trig_label,
                        trigger_body,
                    ));
                }
            }
        }

        if function.commands_enabled {
            let topic_label = format!("{label}_commands");
            let subscription_label = format!("{label}_commands");
            fragment.resource_blocks.push(resource_block(
                "google_pubsub_topic",
                &topic_label,
                [
                    attr(
                        "name",
                        expr::template(format!("${{var.stack_name}}-{}-rq", function.id)),
                    ),
                    attr("project", expr::raw("var.gcp_project")),
                    attr("labels", labels(ctx, "function-commands")),
                ],
            ));

            let mut push_body: Vec<hcl::structure::Structure> = vec![attr(
                "push_endpoint",
                expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
            )];
            if !matches!(function.ingress, Ingress::Public) {
                if let Some(sa) = &service_account {
                    push_body.push(nested(block(
                        "oidc_token",
                        [
                            attr("service_account_email", sa.clone()),
                            attr(
                                "audience",
                                expr::traversal(["google_cloud_run_v2_service", label, "uri"]),
                            ),
                        ],
                    )));
                }
            }

            fragment.resource_blocks.push(resource_block(
                "google_pubsub_subscription",
                &subscription_label,
                [
                    attr(
                        "name",
                        expr::template(format!("${{var.stack_name}}-{}-rq-sub", function.id)),
                    ),
                    attr("project", expr::raw("var.gcp_project")),
                    attr(
                        "topic",
                        expr::traversal(["google_pubsub_topic", &topic_label, "id"]),
                    ),
                    attr(
                        "ack_deadline_seconds",
                        Expression::Number(hcl::Number::from(i64::from(
                            function.timeout_seconds.clamp(10, 600),
                        ))),
                    ),
                    nested(block("push_config", push_body)),
                ],
            ));
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;

        let mut subscription_names: Vec<Expression> = Vec::new();
        let mut scheduler_names: Vec<Expression> = Vec::new();
        let mut eventarc_names: Vec<Expression> = Vec::new();
        for (index, trigger) in function.triggers.iter().enumerate() {
            match trigger {
                FunctionTrigger::Queue { .. } => subscription_names.push(expr::traversal([
                    "google_pubsub_subscription",
                    &format!("{label}_queue_{index}"),
                    "name",
                ])),
                FunctionTrigger::Schedule { .. } => scheduler_names.push(expr::traversal([
                    "google_cloud_scheduler_job",
                    &format!("{label}_schedule_{index}"),
                    "name",
                ])),
                FunctionTrigger::Storage { .. } => eventarc_names.push(expr::traversal([
                    "google_eventarc_trigger",
                    &format!("{label}_storage_{index}"),
                    "name",
                ])),
            }
        }

        let url_field = if matches!(function.ingress, Ingress::Public) {
            expr::traversal(["google_cloud_run_v2_service", label, "uri"])
        } else {
            Expression::Null
        };

        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            ("region", expr::raw("var.gcp_region")),
            (
                "serviceName",
                expr::traversal(["google_cloud_run_v2_service", label, "name"]),
            ),
            ("url", url_field),
            (
                "pubsubSubscriptionNames",
                Expression::Array(subscription_names),
            ),
            ("schedulerJobNames", Expression::Array(scheduler_names)),
            ("eventarcTriggerNames", Expression::Array(eventarc_names)),
            (
                "commandsTopicName",
                if function.commands_enabled {
                    expr::traversal([
                        "google_pubsub_topic",
                        &format!("{label}_commands"),
                        "name",
                    ])
                } else {
                    Expression::Null
                },
            ),
            (
                "commandsSubscriptionName",
                if function.commands_enabled {
                    expr::traversal([
                        "google_pubsub_subscription",
                        &format!("{label}_commands"),
                        "name",
                    ])
                } else {
                    Expression::Null
                },
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let service_url = expr::traversal(["google_cloud_run_v2_service", label, "uri"]);
        let mut fields = vec![
            (
                "service".to_string(),
                Expression::String("cloudrun".to_string()),
            ),
            ("projectId".to_string(), expr::raw("var.gcp_project")),
            (
                "serviceName".to_string(),
                expr::traversal(["google_cloud_run_v2_service", label, "name"]),
            ),
            ("location".to_string(), expr::raw("var.gcp_region")),
            ("privateUrl".to_string(), service_url.clone()),
        ];
        if matches!(function.ingress, Ingress::Public) {
            fields.push(("publicUrl".to_string(), service_url));
        }
        Ok(Some(expr::object(
            fields
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        )))
    }
}
