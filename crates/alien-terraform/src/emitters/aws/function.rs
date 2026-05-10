//! AWS Function — Lambda function plus log group, optional fallback role,
//! and (when `Ingress::Public`) an API Gateway HTTP API in front.

use crate::{
    block::{attr, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::aws::helpers::{
        downcast, jsonencode, label_for_ref, nested_block, private_subnet_ids_expr, required_label,
        security_group_ids_expr, service_account_role_arn, service_assume_role_policy,
        stack_name_template, tags,
    },
    emitters::function_environment::{function_environment, AwsFunctionEnvironmentRenderer},
    expr,
    registry::TfRegistry,
};
use alien_core::{
    crontab_to_eventbridge::crontab_to_eventbridge, import::EmitContext, ErrorData, Function,
    FunctionCode, FunctionTrigger, Ingress, NetworkSettings, Platform, ResourceRef, Result,
    Storage, Vault,
};
use alien_error::AlienError;
use hcl::expr::Expression;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsFunctionEmitter;

impl TfEmitter for AwsFunctionEmitter {
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

        let role_arn = match service_account_role_arn(ctx, &function.permissions) {
            Some(arn) => arn,
            None => {
                let role_label = format!("{label}_role");
                fragment.resource_blocks.push(resource_block(
                    "aws_iam_role",
                    &role_label,
                    [
                        attr("name", stack_name_template(&format!("{}-fn", function.id))),
                        attr(
                            "assume_role_policy",
                            service_assume_role_policy(&["lambda.amazonaws.com"]),
                        ),
                        attr("tags", tags(ctx, "function")),
                    ],
                ));
                fragment.resource_blocks.push(resource_block(
                    "aws_iam_role_policy",
                    &format!("{role_label}_inline"),
                    [
                        attr(
                            "name",
                            Expression::String("alien-managed-policy".to_string()),
                        ),
                        attr("role", expr::traversal(["aws_iam_role", &role_label, "id"])),
                        attr("policy", lambda_fallback_policy(ctx, function)?),
                    ],
                ));
                expr::traversal(["aws_iam_role", &role_label, "arn"])
            }
        };

        let log_group_label = format!("{label}_logs");
        fragment.resource_blocks.push(resource_block(
            "aws_cloudwatch_log_group",
            &log_group_label,
            [
                attr(
                    "name",
                    expr::template(format!("/aws/lambda/${{var.stack_name}}-{}", function.id)),
                ),
                attr(
                    "retention_in_days",
                    Expression::Number(hcl::Number::from(30i64)),
                ),
                attr("tags", tags(ctx, "function")),
            ],
        ));

        let mut function_body = vec![
            attr("function_name", stack_name_template(&function.id)),
            attr("package_type", Expression::String("Image".to_string())),
            attr("image_uri", Expression::String(image.clone())),
            attr("role", role_arn.clone()),
            attr(
                "architectures",
                Expression::Array(vec![Expression::String("arm64".to_string())]),
            ),
            attr(
                "memory_size",
                Expression::Number(hcl::Number::from(i64::from(function.memory_mb))),
            ),
            attr(
                "timeout",
                Expression::Number(hcl::Number::from(i64::from(function.timeout_seconds))),
            ),
        ];
        let env_renderer = AwsFunctionEnvironmentRenderer {
            ctx,
            registry,
            function_id: &function.id,
        };
        let environment_variables = function_environment(function, Platform::Aws, &env_renderer)?;
        function_body.push(nested_block(
            "environment",
            vec![attr(
                "variables",
                expr::object(
                    environment_variables
                        .iter()
                        .map(|(key, value)| (key.as_str(), value.clone())),
                ),
            )],
        ));
        if let Some((subnets, sgs)) = lambda_vpc_config(ctx) {
            function_body.push(nested_block(
                "vpc_config",
                vec![attr("subnet_ids", subnets), attr("security_group_ids", sgs)],
            ));
        }
        function_body.push(attr("tags", tags(ctx, "function")));

        fragment
            .resource_blocks
            .push(resource_block("aws_lambda_function", label, function_body));

        // Triggers + ingress.
        for resource in queue_trigger_resources(ctx, function, label)? {
            fragment.resource_blocks.push(resource);
        }
        for resource in storage_trigger_resources(function, label)? {
            fragment.resource_blocks.push(resource);
        }
        for resource in schedule_trigger_resources(function, label)? {
            fragment.resource_blocks.push(resource);
        }
        if function.ingress == Ingress::Public {
            for resource in public_api_resources(label) {
                fragment.resource_blocks.push(resource);
            }
        }

        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fields: Vec<(String, Expression)> = vec![
            (
                "functionName".to_string(),
                expr::traversal(["aws_lambda_function", label, "function_name"]),
            ),
            (
                "functionArn".to_string(),
                expr::traversal(["aws_lambda_function", label, "arn"]),
            ),
            (
                "eventSourceMappings".to_string(),
                Expression::Array(queue_trigger_uuids(function, label)),
            ),
            (
                "eventbridgeRuleNames".to_string(),
                Expression::Array(schedule_rule_names(function, label)),
            ),
            (
                "s3PermissionStatementIds".to_string(),
                Expression::Array(storage_permission_statement_ids(function, label)),
            ),
            (
                "eventbridgePermissionStatementIds".to_string(),
                Expression::Array(eventbridge_permission_statement_ids(function, label)),
            ),
        ];
        if function.ingress == Ingress::Public {
            fields.push((
                "url".to_string(),
                expr::traversal(["aws_apigatewayv2_api", label, "api_endpoint"]),
            ));
            fields.push((
                "apiId".to_string(),
                expr::traversal(["aws_apigatewayv2_api", label, "id"]),
            ));
            fields.push((
                "integrationId".to_string(),
                expr::traversal(["aws_apigatewayv2_integration", label, "id"]),
            ));
            fields.push((
                "routeId".to_string(),
                expr::traversal(["aws_apigatewayv2_route", label, "id"]),
            ));
            fields.push((
                "stageName".to_string(),
                Expression::String("$default".to_string()),
            ));
        }
        Ok(expr::object(
            fields.iter().map(|(k, v)| (k.as_str(), v.clone())),
        ))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let function = downcast::<Function>(ctx, Function::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fields: Vec<(String, Expression)> = vec![
            (
                "service".to_string(),
                Expression::String("lambda".to_string()),
            ),
            (
                "functionName".to_string(),
                expr::traversal(["aws_lambda_function", label, "function_name"]),
            ),
            (
                "region".to_string(),
                expr::raw("data.aws_region.current.region"),
            ),
        ];
        if function.ingress == Ingress::Public {
            fields.push((
                "url".to_string(),
                expr::traversal(["aws_apigatewayv2_api", label, "api_endpoint"]),
            ));
        }
        Ok(Some(expr::object(
            fields
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        )))
    }
}

fn lambda_fallback_policy(ctx: &EmitContext<'_>, function: &Function) -> Result<Expression> {
    let mut statements = vec![expr::object([
        ("Sid", Expression::String("WriteLogs".to_string())),
        ("Effect", Expression::String("Allow".to_string())),
        (
            "Action",
            Expression::Array(vec![
                Expression::String("logs:CreateLogGroup".to_string()),
                Expression::String("logs:CreateLogStream".to_string()),
                Expression::String("logs:PutLogEvents".to_string()),
            ]),
        ),
        ("Resource", Expression::String("*".to_string())),
    ])];

    for trigger in &function.triggers {
        if let FunctionTrigger::Queue { queue } = trigger {
            let queue_label = label_for_ref(ctx, queue)?;
            statements.push(expr::object([
                ("Sid", Expression::String(format!("ReadQueue{queue_label}"))),
                ("Effect", Expression::String("Allow".to_string())),
                (
                    "Action",
                    Expression::Array(vec![
                        Expression::String("sqs:ReceiveMessage".to_string()),
                        Expression::String("sqs:DeleteMessage".to_string()),
                        Expression::String("sqs:GetQueueAttributes".to_string()),
                        Expression::String("sqs:ChangeMessageVisibility".to_string()),
                    ]),
                ),
                (
                    "Resource",
                    expr::traversal(["aws_sqs_queue", queue_label, "arn"]),
                ),
            ]));
        }
    }

    for link in &function.links {
        statements.extend(link_permission_statements(ctx, link)?);
    }

    Ok(jsonencode(expr::object([
        ("Version", Expression::String("2012-10-17".to_string())),
        ("Statement", Expression::Array(statements)),
    ])))
}

fn link_permission_statements(
    ctx: &EmitContext<'_>,
    link: &ResourceRef,
) -> Result<Vec<Expression>> {
    let label = label_for_ref(ctx, link)?;
    let label_owned = label.to_string();
    if link.resource_type == Storage::RESOURCE_TYPE {
        Ok(vec![expr::object([
            (
                "Sid",
                Expression::String(format!("AccessStorage{label_owned}")),
            ),
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("s3:GetObject".to_string()),
                    Expression::String("s3:PutObject".to_string()),
                    Expression::String("s3:DeleteObject".to_string()),
                    Expression::String("s3:ListBucket".to_string()),
                ]),
            ),
            (
                "Resource",
                Expression::Array(vec![
                    expr::traversal(["aws_s3_bucket", &label_owned, "arn"]),
                    expr::raw(format!("\"${{aws_s3_bucket.{label_owned}.arn}}/*\"")),
                ]),
            ),
        ])])
    } else if link.resource_type == alien_core::Queue::RESOURCE_TYPE {
        Ok(vec![expr::object([
            (
                "Sid",
                Expression::String(format!("AccessQueue{label_owned}")),
            ),
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("sqs:SendMessage".to_string()),
                    Expression::String("sqs:ReceiveMessage".to_string()),
                    Expression::String("sqs:DeleteMessage".to_string()),
                    Expression::String("sqs:GetQueueAttributes".to_string()),
                ]),
            ),
            (
                "Resource",
                expr::traversal(["aws_sqs_queue", &label_owned, "arn"]),
            ),
        ])])
    } else if link.resource_type == alien_core::Kv::RESOURCE_TYPE {
        Ok(vec![expr::object([
            (
                "Sid",
                Expression::String(format!("AccessTable{label_owned}")),
            ),
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("dynamodb:GetItem".to_string()),
                    Expression::String("dynamodb:PutItem".to_string()),
                    Expression::String("dynamodb:UpdateItem".to_string()),
                    Expression::String("dynamodb:DeleteItem".to_string()),
                    Expression::String("dynamodb:Query".to_string()),
                    Expression::String("dynamodb:Scan".to_string()),
                ]),
            ),
            (
                "Resource",
                expr::traversal(["aws_dynamodb_table", &label_owned, "arn"]),
            ),
        ])])
    } else if link.resource_type == Vault::RESOURCE_TYPE {
        Ok(vec![expr::object([
            (
                "Sid",
                Expression::String(format!("AccessVault{label_owned}")),
            ),
            ("Effect", Expression::String("Allow".to_string())),
            (
                "Action",
                Expression::Array(vec![
                    Expression::String("ssm:GetParameter".to_string()),
                    Expression::String("ssm:GetParameters".to_string()),
                    Expression::String("ssm:PutParameter".to_string()),
                    Expression::String("ssm:DeleteParameter".to_string()),
                ]),
            ),
            (
                "Resource",
                expr::template(format!(
                    "arn:aws:ssm:${{data.aws_region.current.region}}:${{data.aws_caller_identity.current.account_id}}:parameter/${{var.stack_name}}-{}/*",
                    link.id
                )),
            ),
        ])])
    } else {
        Ok(vec![])
    }
}

fn lambda_vpc_config(ctx: &EmitContext<'_>) -> Option<(Expression, Expression)> {
    let (_label, network) =
        crate::emitters::aws::helpers::default_network(ctx).map(|(l, n)| (l, n))?;
    match &network.settings {
        NetworkSettings::Create { .. } | NetworkSettings::ByoVpcAws { .. } => {
            Some((private_subnet_ids_expr(ctx), security_group_ids_expr(ctx)))
        }
        _ => None,
    }
}

fn queue_trigger_resources(
    ctx: &EmitContext<'_>,
    function: &Function,
    label: &str,
) -> Result<Vec<hcl::structure::Block>> {
    let mut resources = Vec::new();
    let mut index = 0usize;
    for trigger in &function.triggers {
        let FunctionTrigger::Queue { queue } = trigger else {
            continue;
        };
        let queue_label = label_for_ref(ctx, queue)?;
        resources.push(resource_block(
            "aws_lambda_event_source_mapping",
            &format!("{label}_queue_{index}"),
            [
                attr(
                    "event_source_arn",
                    expr::traversal(["aws_sqs_queue", queue_label, "arn"]),
                ),
                attr(
                    "function_name",
                    expr::traversal(["aws_lambda_function", label, "function_name"]),
                ),
                attr("batch_size", Expression::Number(hcl::Number::from(1i64))),
                attr("enabled", Expression::Bool(true)),
            ],
        ));
        index += 1;
    }
    Ok(resources)
}

fn storage_trigger_resources(
    function: &Function,
    label: &str,
) -> Result<Vec<hcl::structure::Block>> {
    let mut resources = Vec::new();
    for trigger in &function.triggers {
        let FunctionTrigger::Storage { storage, .. } = trigger else {
            continue;
        };
        if storage.resource_type != Storage::RESOURCE_TYPE {
            continue;
        }
        let stmt_id = format!("{label}_storage_{}", storage.id);
        resources.push(resource_block(
            "aws_lambda_permission",
            &stmt_id,
            [
                attr("statement_id", stack_name_template(&stmt_id)),
                attr(
                    "action",
                    Expression::String("lambda:InvokeFunction".to_string()),
                ),
                attr(
                    "function_name",
                    expr::traversal(["aws_lambda_function", label, "function_name"]),
                ),
                attr(
                    "principal",
                    Expression::String("s3.amazonaws.com".to_string()),
                ),
                attr(
                    "source_account",
                    expr::raw("data.aws_caller_identity.current.account_id"),
                ),
            ],
        ));
    }
    Ok(resources)
}

fn schedule_trigger_resources(
    function: &Function,
    label: &str,
) -> Result<Vec<hcl::structure::Block>> {
    let mut resources = Vec::new();
    let mut index = 0usize;
    for trigger in &function.triggers {
        let FunctionTrigger::Schedule { cron } = trigger else {
            continue;
        };
        let schedule = crontab_to_eventbridge(cron).map_err(|reason| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "invalid schedule trigger for function '{}': {}",
                    function.id, reason
                ),
            })
        })?;
        let rule_label = format!("{label}_schedule_{index}");
        let target_label = format!("{label}_schedule_target_{index}");
        let perm_label = format!("{label}_schedule_perm_{index}");
        resources.push(resource_block(
            "aws_cloudwatch_event_rule",
            &rule_label,
            [
                attr(
                    "name",
                    stack_name_template(&format!("{}-schedule-{index}", function.id)),
                ),
                attr("schedule_expression", Expression::String(schedule.clone())),
                attr("state", Expression::String("ENABLED".to_string())),
                attr(
                    "tags",
                    expr::object([("ManagedBy", Expression::String("alien.dev".to_string()))]),
                ),
            ],
        ));
        resources.push(resource_block(
            "aws_cloudwatch_event_target",
            &target_label,
            [
                attr(
                    "rule",
                    expr::traversal(["aws_cloudwatch_event_rule", &rule_label, "name"]),
                ),
                attr(
                    "target_id",
                    Expression::String(format!("{label}-target-{index}")),
                ),
                attr(
                    "arn",
                    expr::traversal(["aws_lambda_function", label, "arn"]),
                ),
            ],
        ));
        resources.push(resource_block(
            "aws_lambda_permission",
            &perm_label,
            [
                attr(
                    "statement_id",
                    Expression::String(format!("{label}ScheduleInvoke{index}")),
                ),
                attr(
                    "action",
                    Expression::String("lambda:InvokeFunction".to_string()),
                ),
                attr(
                    "function_name",
                    expr::traversal(["aws_lambda_function", label, "function_name"]),
                ),
                attr(
                    "principal",
                    Expression::String("events.amazonaws.com".to_string()),
                ),
                attr(
                    "source_arn",
                    expr::traversal(["aws_cloudwatch_event_rule", &rule_label, "arn"]),
                ),
            ],
        ));
        index += 1;
    }
    Ok(resources)
}

fn public_api_resources(label: &str) -> Vec<hcl::structure::Block> {
    vec![
        resource_block(
            "aws_apigatewayv2_api",
            label,
            [
                attr("name", stack_name_template(&format!("{label}-api"))),
                attr("protocol_type", Expression::String("HTTP".to_string())),
            ],
        ),
        resource_block(
            "aws_apigatewayv2_integration",
            label,
            [
                attr(
                    "api_id",
                    expr::traversal(["aws_apigatewayv2_api", label, "id"]),
                ),
                attr(
                    "integration_type",
                    Expression::String("AWS_PROXY".to_string()),
                ),
                attr("integration_method", Expression::String("POST".to_string())),
                attr(
                    "integration_uri",
                    expr::traversal(["aws_lambda_function", label, "invoke_arn"]),
                ),
                attr(
                    "payload_format_version",
                    Expression::String("2.0".to_string()),
                ),
            ],
        ),
        resource_block(
            "aws_apigatewayv2_route",
            label,
            [
                attr(
                    "api_id",
                    expr::traversal(["aws_apigatewayv2_api", label, "id"]),
                ),
                attr("route_key", Expression::String("$default".to_string())),
                attr(
                    "target",
                    expr::template(format!(
                        "integrations/${{aws_apigatewayv2_integration.{label}.id}}"
                    )),
                ),
            ],
        ),
        resource_block(
            "aws_apigatewayv2_stage",
            label,
            [
                attr(
                    "api_id",
                    expr::traversal(["aws_apigatewayv2_api", label, "id"]),
                ),
                attr("name", Expression::String("$default".to_string())),
                attr("auto_deploy", Expression::Bool(true)),
            ],
        ),
        resource_block(
            "aws_lambda_permission",
            &format!("{label}_api"),
            [
                attr(
                    "statement_id",
                    Expression::String(format!("{label}ApiInvoke")),
                ),
                attr(
                    "action",
                    Expression::String("lambda:InvokeFunction".to_string()),
                ),
                attr(
                    "function_name",
                    expr::traversal(["aws_lambda_function", label, "function_name"]),
                ),
                attr(
                    "principal",
                    Expression::String("apigateway.amazonaws.com".to_string()),
                ),
                attr(
                    "source_arn",
                    expr::template(format!(
                        "${{aws_apigatewayv2_api.{label}.execution_arn}}/*/*"
                    )),
                ),
            ],
        ),
    ]
}

fn queue_trigger_uuids(function: &Function, label: &str) -> Vec<Expression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, FunctionTrigger::Queue { .. }) {
            ids.push(expr::traversal([
                "aws_lambda_event_source_mapping",
                &format!("{label}_queue_{index}"),
                "uuid",
            ]));
            index += 1;
        }
    }
    ids
}

fn schedule_rule_names(function: &Function, label: &str) -> Vec<Expression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, FunctionTrigger::Schedule { .. }) {
            ids.push(expr::traversal([
                "aws_cloudwatch_event_rule",
                &format!("{label}_schedule_{index}"),
                "name",
            ]));
            index += 1;
        }
    }
    ids
}

fn storage_permission_statement_ids(function: &Function, label: &str) -> Vec<Expression> {
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        let FunctionTrigger::Storage { storage, .. } = trigger else {
            continue;
        };
        ids.push(expr::traversal([
            "aws_lambda_permission",
            &format!("{label}_storage_{}", storage.id),
            "statement_id",
        ]));
    }
    ids
}

fn eventbridge_permission_statement_ids(function: &Function, label: &str) -> Vec<Expression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, FunctionTrigger::Schedule { .. }) {
            ids.push(expr::traversal([
                "aws_lambda_permission",
                &format!("{label}_schedule_perm_{index}"),
                "statement_id",
            ]));
            index += 1;
        }
    }
    ids
}
