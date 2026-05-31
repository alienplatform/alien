//! AWS Worker — Lambda function plus log group, optional fallback role,
//! and optional API Gateway HTTP API for public ingress.
//!
//! Triggers:
//!
//! * Queue: `AWS::Lambda::EventSourceMapping`.
//! * Storage: `AWS::Lambda::Permission` (the bucket's notification config
//!   is wired by the storage emitter).
//! * Schedule: `AWS::Events::Rule` + `AWS::Lambda::Permission`.
//!
//! `Ingress::Public` adds a private API Gateway HTTP API in front of the
//! Lambda with proxy integration.

use crate::{
    emitter::CfEmitter,
    emitters::aws::helpers::{
        logical_id_for_ref, private_subnet_ids_expr, required_logical_id, resource_config,
        role_for_profile_or_fallback, security_group_ids_expr, stack_name, tags,
    },
    registry::CfRegistry,
    template::{CfExpression, CfResource},
};
use alien_core::{
    crontab_to_eventbridge::crontab_to_eventbridge, import::EmitContext,
    render_runtime_environment_plan, validate_runtime_environment_user_map,
    worker_runtime_environment_contract, ErrorData, Ingress, NetworkSettings, Platform, Result,
    RuntimeEnvironmentBindingEntry, RuntimeEnvironmentBindingSource, RuntimeEnvironmentRenderer,
    RuntimeEnvironmentValue, Storage, Worker, WorkerCode, WorkerTrigger,
};
use alien_error::AlienError;
use indexmap::IndexMap;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsWorkerEmitter;

impl CfEmitter for AwsWorkerEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let registry = CfRegistry::built_in();
        self.emit_resources_with_registry(ctx, &registry)
    }

    fn emit_resources_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &CfRegistry,
    ) -> Result<Vec<CfResource>> {
        let function = resource_config::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let WorkerCode::Image { image } = &function.code else {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_cloudformation_template".to_string(),
                reason: format!(
                    "worker '{}' uses source code; CloudFormation templates require a pre-built image",
                    function.id
                ),
            }));
        };

        let logical_id = required_logical_id(ctx)?;
        let role = role_for_profile_or_fallback(
            ctx,
            &function.permissions,
            &format!("{logical_id}Role"),
            "lambda.amazonaws.com",
            lambda_fallback_policy(),
        )?;

        let log_group_id = format!("{logical_id}LogGroup");
        let mut log_group =
            CfResource::new(log_group_id.clone(), "AWS::Logs::LogGroup".to_string());
        log_group.properties.insert(
            "LogGroupName".to_string(),
            CfExpression::sub(format!("/aws/lambda/${{AWS::StackName}}-{}", function.id)),
        );
        log_group
            .properties
            .insert("RetentionInDays".to_string(), CfExpression::Integer(30));
        log_group.deletion_policy = Some("Retain".to_string());
        log_group.update_replace_policy = Some("Retain".to_string());

        let mut lambda =
            CfResource::new(logical_id.to_string(), "AWS::Lambda::Function".to_string());
        lambda
            .properties
            .insert("FunctionName".to_string(), stack_name(&function.id));
        lambda
            .properties
            .insert("PackageType".to_string(), CfExpression::from("Image"));
        lambda.properties.insert(
            "Architectures".to_string(),
            CfExpression::list([CfExpression::from("arm64")]),
        );
        lambda.properties.insert(
            "Code".to_string(),
            CfExpression::object([("ImageUri", CfExpression::from(image.clone()))]),
        );
        lambda
            .properties
            .insert("Role".to_string(), role.arn.clone());
        lambda.properties.insert(
            "MemorySize".to_string(),
            CfExpression::from(function.memory_mb),
        );
        lambda.properties.insert(
            "Timeout".to_string(),
            CfExpression::from(function.timeout_seconds),
        );
        lambda.properties.insert(
            "Environment".to_string(),
            CfExpression::object([(
                "Variables",
                CfExpression::Object(worker_environment(ctx, registry, function)?),
            )]),
        );
        if let Some(vpc_config) = lambda_vpc_config(ctx) {
            lambda
                .properties
                .insert("VpcConfig".to_string(), vpc_config);
        }
        lambda.properties.insert("Tags".to_string(), tags(ctx));
        lambda.depends_on.push(log_group_id.clone());
        if let Some(role_id) = role.resource_id.clone() {
            lambda.depends_on.push(role_id);
        }

        let mut resources = role.resources;
        resources.push(log_group);
        resources.push(lambda);
        resources.extend(queue_trigger_resources(ctx, function, logical_id)?);
        resources.extend(storage_trigger_permissions(ctx, function, logical_id)?);
        resources.extend(schedule_trigger_resources(function, logical_id)?);
        if function.ingress == Ingress::Public {
            resources.extend(public_api_resources(logical_id));
        }

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let function = resource_config::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let mut fields = vec![
            ("functionName", CfExpression::ref_(logical_id)),
            ("functionArn", CfExpression::get_att(logical_id, "Arn")),
            (
                "eventSourceMappings",
                CfExpression::list(queue_trigger_ids(function, logical_id)),
            ),
            (
                "eventbridgeRuleNames",
                CfExpression::list(schedule_rule_names(function, logical_id)),
            ),
            (
                "s3PermissionStatementIds",
                CfExpression::list(storage_permission_statement_ids(function, logical_id)),
            ),
            (
                "eventbridgePermissionStatementIds",
                CfExpression::list(eventbridge_permission_statement_ids(function, logical_id)),
            ),
        ];
        if function.ingress == Ingress::Public {
            fields.extend([
                (
                    "url",
                    CfExpression::sub(format!(
                        "https://${{{logical_id}Api}}.execute-api.${{AWS::Region}}.${{AWS::URLSuffix}}"
                    )),
                ),
                ("apiId", CfExpression::ref_(format!("{logical_id}Api"))),
                (
                    "integrationId",
                    CfExpression::ref_(format!("{logical_id}Integration")),
                ),
                ("routeId", CfExpression::ref_(format!("{logical_id}Route"))),
                ("stageName", CfExpression::from("$default")),
            ]);
        }

        Ok(CfExpression::object(fields))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let function = resource_config::<Worker>(ctx, Worker::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let mut fields = vec![
            ("service", CfExpression::from("lambda")),
            ("functionName", CfExpression::ref_(logical_id)),
            ("region", CfExpression::ref_("AWS::Region")),
        ];
        if function.ingress == Ingress::Public {
            fields.push((
                "url",
                CfExpression::sub(format!(
                    "https://${{{logical_id}Api}}.execute-api.${{AWS::Region}}.${{AWS::URLSuffix}}"
                )),
            ));
        }

        Ok(Some(CfExpression::object(fields)))
    }
}

struct AwsWorkerEnvironmentRenderer<'a> {
    ctx: &'a EmitContext<'a>,
    registry: &'a CfRegistry,
    worker_id: &'a str,
}

impl RuntimeEnvironmentRenderer for AwsWorkerEnvironmentRenderer<'_> {
    type Value = CfExpression;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> Result<Option<Self::Value>> {
        match value {
            RuntimeEnvironmentValue::Literal(value) => {
                Ok(Some(CfExpression::from(value.to_string())))
            }
            RuntimeEnvironmentValue::AwsAccountId => Ok(Some(CfExpression::ref_("AWS::AccountId"))),
            RuntimeEnvironmentValue::AwsRegion => Ok(Some(CfExpression::ref_("AWS::Region"))),
            RuntimeEnvironmentValue::CurrentWorkerBindingName => {
                Ok(Some(CfExpression::from(self.worker_id.to_string())))
            }
            RuntimeEnvironmentValue::AzureClientId
            | RuntimeEnvironmentValue::AzureRegion
            | RuntimeEnvironmentValue::AzureSubscriptionId
            | RuntimeEnvironmentValue::AzureTenantId
            | RuntimeEnvironmentValue::BasePlatform
            | RuntimeEnvironmentValue::CurrentContainerBindingName
            | RuntimeEnvironmentValue::GcpProjectId
            | RuntimeEnvironmentValue::GcpRegion => Ok(None),
        }
    }

    fn render_runtime_environment_binding(
        &self,
        entry: &RuntimeEnvironmentBindingEntry,
    ) -> Result<Option<Self::Value>> {
        render_linked_binding(self.ctx, self.registry, entry)
    }
}

fn worker_environment(
    ctx: &EmitContext<'_>,
    registry: &CfRegistry,
    function: &Worker,
) -> Result<IndexMap<String, CfExpression>> {
    validate_runtime_environment_user_map(&function.environment)?;
    let renderer = AwsWorkerEnvironmentRenderer {
        ctx,
        registry,
        worker_id: &function.id,
    };
    let plan = worker_runtime_environment_contract(Platform::Aws, &function.id, &function.links);
    let mut env = render_runtime_environment_plan(&plan, &renderer)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    for (key, value) in &function.environment {
        env.insert(key.clone(), CfExpression::from(value.clone()));
    }

    Ok(env.into_iter().collect())
}

fn render_linked_binding(
    ctx: &EmitContext<'_>,
    registry: &CfRegistry,
    entry: &RuntimeEnvironmentBindingEntry,
) -> Result<Option<CfExpression>> {
    match &entry.source {
        RuntimeEnvironmentBindingSource::CurrentContainer
        | RuntimeEnvironmentBindingSource::CurrentWorker => Ok(None),
        RuntimeEnvironmentBindingSource::LinkedResource(link) => {
            let resource = ctx.stack.resources.get(link.id()).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "worker '{}' links to missing resource '{}'",
                        ctx.resource_id,
                        link.id()
                    ),
                })
            })?;
            let actual_type = resource.config.resource_type();
            if actual_type != link.resource_type {
                return Err(AlienError::new(ErrorData::UnexpectedResourceType {
                    resource_id: link.id().to_string(),
                    expected: link.resource_type.clone(),
                    actual: actual_type,
                }));
            }

            let linked_ctx = EmitContext {
                stack: ctx.stack,
                resource,
                resource_id: link.id(),
                platform: ctx.platform,
                stack_settings: ctx.stack_settings,
                names: ctx.names,
            };
            let emitter = registry.require(&link.resource_type, ctx.platform)?;
            let binding_ref = emitter.emit_binding_ref(&linked_ctx)?.ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "CloudFormation emitter for resource '{}' ({}) does not provide a runtime binding",
                        link.id(),
                        link.resource_type
                    ),
                })
            })?;
            Ok(Some(CfExpression::to_json_string(binding_ref)))
        }
    }
}

fn lambda_fallback_policy() -> CfExpression {
    let statements = vec![CfExpression::object([
        ("Sid", CfExpression::from("WriteLogs")),
        ("Effect", CfExpression::from("Allow")),
        (
            "Action",
            CfExpression::list([
                CfExpression::from("logs:CreateLogGroup"),
                CfExpression::from("logs:CreateLogStream"),
                CfExpression::from("logs:PutLogEvents"),
            ]),
        ),
        ("Resource", CfExpression::from("*")),
    ])];

    CfExpression::object([
        ("Version", CfExpression::from("2012-10-17")),
        ("Statement", CfExpression::list(statements)),
    ])
}

fn lambda_vpc_config(ctx: &EmitContext<'_>) -> Option<CfExpression> {
    let (_network_id, network) =
        crate::emitters::aws::helpers::default_network(ctx).map(|(id, network)| (id, network))?;
    match &network.settings {
        NetworkSettings::UseDefault => None,
        NetworkSettings::Create { .. } | NetworkSettings::ByoVpcAws { .. } => {
            Some(CfExpression::object([
                ("SubnetIds", private_subnet_ids_expr(ctx)),
                ("SecurityGroupIds", security_group_ids_expr(ctx)),
            ]))
        }
        NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => None,
    }
}

fn queue_trigger_resources(
    ctx: &EmitContext<'_>,
    function: &Worker,
    logical_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let mut queue_index = 0usize;
    for trigger in &function.triggers {
        let WorkerTrigger::Queue { queue } = trigger else {
            continue;
        };
        let queue_id = logical_id_for_ref(ctx, queue)?;
        let mapping_id = format!("{logical_id}QueueTrigger{queue_index}");
        let mut mapping =
            CfResource::new(mapping_id, "AWS::Lambda::EventSourceMapping".to_string());
        mapping
            .properties
            .insert("BatchSize".to_string(), CfExpression::Integer(1));
        mapping
            .properties
            .insert("Enabled".to_string(), CfExpression::from(true));
        mapping.properties.insert(
            "EventSourceArn".to_string(),
            CfExpression::get_att(queue_id, "Arn"),
        );
        mapping
            .properties
            .insert("FunctionName".to_string(), CfExpression::ref_(logical_id));
        resources.push(mapping);
        queue_index += 1;
    }
    Ok(resources)
}

fn storage_trigger_permissions(
    _ctx: &EmitContext<'_>,
    function: &Worker,
    logical_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    for trigger in &function.triggers {
        let WorkerTrigger::Storage { storage, .. } = trigger else {
            continue;
        };
        if storage.resource_type != Storage::RESOURCE_TYPE {
            continue;
        }
        let statement_id = format!("{logical_id}StoragePermission{}", storage.id);
        let mut permission =
            CfResource::new(statement_id.clone(), "AWS::Lambda::Permission".to_string());
        permission.properties.insert(
            "Action".to_string(),
            CfExpression::from("lambda:InvokeFunction"),
        );
        permission
            .properties
            .insert("FunctionName".to_string(), CfExpression::ref_(logical_id));
        permission.properties.insert(
            "Principal".to_string(),
            CfExpression::from("s3.amazonaws.com"),
        );
        permission.properties.insert(
            "SourceAccount".to_string(),
            CfExpression::ref_("AWS::AccountId"),
        );
        resources.push(permission);
    }
    Ok(resources)
}

fn schedule_trigger_resources(function: &Worker, logical_id: &str) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let mut schedule_index = 0usize;
    for trigger in &function.triggers {
        let WorkerTrigger::Schedule { cron } = trigger else {
            continue;
        };
        let schedule_expression = crontab_to_eventbridge(cron).map_err(|reason| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "invalid schedule trigger for worker '{}': {}",
                    function.id, reason
                ),
            })
        })?;
        let rule_id = format!("{logical_id}ScheduleTrigger{schedule_index}");
        let permission_id = format!("{logical_id}SchedulePermission{schedule_index}");

        let mut rule = CfResource::new(rule_id.clone(), "AWS::Events::Rule".to_string());
        rule.properties.insert(
            "ScheduleExpression".to_string(),
            CfExpression::from(schedule_expression),
        );
        rule.properties
            .insert("State".to_string(), CfExpression::from("ENABLED"));
        rule.properties.insert(
            "Targets".to_string(),
            CfExpression::list([CfExpression::object([
                (
                    "Id",
                    CfExpression::from(format!("{logical_id}Target{schedule_index}")),
                ),
                ("Arn", CfExpression::get_att(logical_id, "Arn")),
            ])]),
        );

        let mut permission = CfResource::new(permission_id, "AWS::Lambda::Permission".to_string());
        permission.properties.insert(
            "Action".to_string(),
            CfExpression::from("lambda:InvokeFunction"),
        );
        permission
            .properties
            .insert("FunctionName".to_string(), CfExpression::ref_(logical_id));
        permission.properties.insert(
            "Principal".to_string(),
            CfExpression::from("events.amazonaws.com"),
        );
        permission.properties.insert(
            "SourceArn".to_string(),
            CfExpression::get_att(&rule_id, "Arn"),
        );

        resources.push(rule);
        resources.push(permission);
        schedule_index += 1;
    }
    Ok(resources)
}

fn public_api_resources(logical_id: &str) -> Vec<CfResource> {
    let api_id = format!("{logical_id}Api");
    let integration_id = format!("{logical_id}Integration");
    let route_id = format!("{logical_id}Route");
    let stage_id = format!("{logical_id}Stage");
    let permission_id = format!("{logical_id}ApiPermission");

    let mut api = CfResource::new(api_id.clone(), "AWS::ApiGatewayV2::Api".to_string());
    api.properties.insert(
        "Name".to_string(),
        CfExpression::sub(format!("${{AWS::StackName}}-{logical_id}")),
    );
    api.properties
        .insert("ProtocolType".to_string(), CfExpression::from("HTTP"));
    api.properties.insert(
        "DisableExecuteApiEndpoint".to_string(),
        CfExpression::from(false),
    );

    let mut integration = CfResource::new(
        integration_id.clone(),
        "AWS::ApiGatewayV2::Integration".to_string(),
    );
    integration
        .properties
        .insert("ApiId".to_string(), CfExpression::ref_(&api_id));
    integration.properties.insert(
        "IntegrationType".to_string(),
        CfExpression::from("AWS_PROXY"),
    );
    integration
        .properties
        .insert("IntegrationMethod".to_string(), CfExpression::from("POST"));
    integration.properties.insert(
        "IntegrationUri".to_string(),
        CfExpression::sub(format!(
            "arn:${{AWS::Partition}}:apigateway:${{AWS::Region}}:lambda:path/2015-03-31/functions/${{{logical_id}.Arn}}/invocations"
        )),
    );
    integration.properties.insert(
        "PayloadFormatVersion".to_string(),
        CfExpression::from("2.0"),
    );

    let mut route = CfResource::new(route_id.clone(), "AWS::ApiGatewayV2::Route".to_string());
    route
        .properties
        .insert("ApiId".to_string(), CfExpression::ref_(&api_id));
    route
        .properties
        .insert("RouteKey".to_string(), CfExpression::from("$default"));
    route.properties.insert(
        "Target".to_string(),
        CfExpression::sub(format!("integrations/${{{integration_id}}}")),
    );

    let mut stage = CfResource::new(stage_id, "AWS::ApiGatewayV2::Stage".to_string());
    stage
        .properties
        .insert("ApiId".to_string(), CfExpression::ref_(&api_id));
    stage
        .properties
        .insert("StageName".to_string(), CfExpression::from("$default"));
    stage
        .properties
        .insert("AutoDeploy".to_string(), CfExpression::from(true));

    let mut permission = CfResource::new(permission_id, "AWS::Lambda::Permission".to_string());
    permission.properties.insert(
        "Action".to_string(),
        CfExpression::from("lambda:InvokeFunction"),
    );
    permission
        .properties
        .insert("FunctionName".to_string(), CfExpression::ref_(logical_id));
    permission.properties.insert(
        "Principal".to_string(),
        CfExpression::from("apigateway.amazonaws.com"),
    );
    permission.properties.insert(
        "SourceArn".to_string(),
        CfExpression::sub(format!(
            "arn:${{AWS::Partition}}:execute-api:${{AWS::Region}}:${{AWS::AccountId}}:${{{api_id}}}/*/*"
        )),
    );

    vec![api, integration, route, stage, permission]
}

fn queue_trigger_ids(function: &Worker, logical_id: &str) -> Vec<CfExpression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, WorkerTrigger::Queue { .. }) {
            ids.push(CfExpression::ref_(format!(
                "{logical_id}QueueTrigger{index}"
            )));
            index += 1;
        }
    }
    ids
}

fn schedule_rule_names(function: &Worker, logical_id: &str) -> Vec<CfExpression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, WorkerTrigger::Schedule { .. }) {
            ids.push(CfExpression::ref_(format!(
                "{logical_id}ScheduleTrigger{index}"
            )));
            index += 1;
        }
    }
    ids
}

fn storage_permission_statement_ids(function: &Worker, logical_id: &str) -> Vec<CfExpression> {
    function
        .triggers
        .iter()
        .filter_map(|trigger| {
            let WorkerTrigger::Storage { storage, .. } = trigger else {
                return None;
            };
            Some(CfExpression::from(format!(
                "{logical_id}StoragePermission{}",
                storage.id
            )))
        })
        .collect()
}

fn eventbridge_permission_statement_ids(function: &Worker, logical_id: &str) -> Vec<CfExpression> {
    let mut index = 0usize;
    let mut ids = Vec::new();
    for trigger in &function.triggers {
        if matches!(trigger, WorkerTrigger::Schedule { .. }) {
            ids.push(CfExpression::from(schedule_permission_statement_id(
                logical_id, index,
            )));
            index += 1;
        }
    }
    ids
}

fn schedule_permission_statement_id(logical_id: &str, index: usize) -> String {
    format!("{logical_id}ScheduleInvoke{index}")
}
