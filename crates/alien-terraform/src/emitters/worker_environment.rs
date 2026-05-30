use std::collections::BTreeMap;

use crate::{expr, registry::TfRegistry};
use alien_core::{
    import::EmitContext, render_runtime_environment_plan, validate_runtime_environment_user_map,
    worker_runtime_environment_contract, ErrorData, Platform, RuntimeEnvironmentBindingEntry,
    RuntimeEnvironmentBindingSource, RuntimeEnvironmentRenderer, RuntimeEnvironmentValue, Worker,
};
use alien_error::AlienError;
use hcl::expr::Expression;

pub fn worker_environment<R>(
    worker: &Worker,
    platform: Platform,
    renderer: &R,
) -> alien_core::Result<BTreeMap<String, Expression>>
where
    R: RuntimeEnvironmentRenderer<Value = Expression>,
{
    validate_runtime_environment_user_map(&worker.environment)?;
    let plan = worker_runtime_environment_contract(platform, &worker.id, &worker.links);
    let mut env = render_runtime_environment_plan(&plan, renderer)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    for (key, value) in &worker.environment {
        env.insert(key.clone(), Expression::String(value.clone()));
    }

    Ok(env)
}

fn render_linked_binding(
    ctx: &EmitContext<'_>,
    registry: &TfRegistry,
    entry: &RuntimeEnvironmentBindingEntry,
) -> alien_core::Result<Option<Expression>> {
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
                        "terraform emitter for resource '{}' ({}) does not provide a runtime binding",
                        link.id(),
                        link.resource_type
                    ),
                })
            })?;
            Ok(Some(expr::jsonencode(binding_ref)))
        }
    }
}

pub struct AwsWorkerEnvironmentRenderer<'a, 'ctx> {
    pub ctx: &'a EmitContext<'ctx>,
    pub registry: &'a TfRegistry,
    pub worker_id: &'a str,
}

impl RuntimeEnvironmentRenderer for AwsWorkerEnvironmentRenderer<'_, '_> {
    type Value = Expression;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> alien_core::Result<Option<Self::Value>> {
        match value {
            RuntimeEnvironmentValue::Literal(value) => {
                Ok(Some(Expression::String(value.to_string())))
            }
            RuntimeEnvironmentValue::AwsAccountId => Ok(Some(expr::raw(
                "data.aws_caller_identity.current.account_id",
            ))),
            RuntimeEnvironmentValue::AwsRegion => {
                Ok(Some(expr::raw("data.aws_region.current.region")))
            }
            RuntimeEnvironmentValue::CurrentWorkerBindingName => {
                Ok(Some(Expression::String(self.worker_id.to_string())))
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
    ) -> alien_core::Result<Option<Self::Value>> {
        render_linked_binding(self.ctx, self.registry, entry)
    }
}

pub struct GcpWorkerEnvironmentRenderer<'a, 'ctx> {
    pub ctx: &'a EmitContext<'ctx>,
    pub registry: &'a TfRegistry,
    pub worker_id: &'a str,
}

impl RuntimeEnvironmentRenderer for GcpWorkerEnvironmentRenderer<'_, '_> {
    type Value = Expression;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> alien_core::Result<Option<Self::Value>> {
        match value {
            RuntimeEnvironmentValue::Literal(value) => {
                Ok(Some(Expression::String(value.to_string())))
            }
            RuntimeEnvironmentValue::CurrentWorkerBindingName => {
                Ok(Some(Expression::String(self.worker_id.to_string())))
            }
            RuntimeEnvironmentValue::GcpProjectId => Ok(Some(expr::raw("var.gcp_project"))),
            RuntimeEnvironmentValue::GcpRegion => Ok(Some(expr::raw("var.gcp_region"))),
            RuntimeEnvironmentValue::AwsAccountId
            | RuntimeEnvironmentValue::AwsRegion
            | RuntimeEnvironmentValue::AzureClientId
            | RuntimeEnvironmentValue::AzureRegion
            | RuntimeEnvironmentValue::AzureSubscriptionId
            | RuntimeEnvironmentValue::AzureTenantId
            | RuntimeEnvironmentValue::BasePlatform
            | RuntimeEnvironmentValue::CurrentContainerBindingName => Ok(None),
        }
    }

    fn render_runtime_environment_binding(
        &self,
        entry: &RuntimeEnvironmentBindingEntry,
    ) -> alien_core::Result<Option<Self::Value>> {
        render_linked_binding(self.ctx, self.registry, entry)
    }
}

pub struct AzureWorkerEnvironmentRenderer<'a, 'ctx> {
    pub ctx: &'a EmitContext<'ctx>,
    pub registry: &'a TfRegistry,
    pub worker_id: &'a str,
    pub client_config_label: &'a str,
    pub service_account_label: Option<&'a str>,
}

impl RuntimeEnvironmentRenderer for AzureWorkerEnvironmentRenderer<'_, '_> {
    type Value = Expression;

    fn render_runtime_environment_value(
        &self,
        value: RuntimeEnvironmentValue,
    ) -> alien_core::Result<Option<Self::Value>> {
        match value {
            RuntimeEnvironmentValue::Literal(value) => {
                Ok(Some(Expression::String(value.to_string())))
            }
            RuntimeEnvironmentValue::AzureClientId => Ok(self.service_account_label.map(|label| {
                expr::traversal(["azurerm_user_assigned_identity", label, "client_id"])
            })),
            RuntimeEnvironmentValue::AzureRegion => Ok(Some(expr::raw("var.azure_location"))),
            RuntimeEnvironmentValue::AzureSubscriptionId => {
                Ok(Some(expr::raw("var.azure_subscription_id")))
            }
            RuntimeEnvironmentValue::AzureTenantId => Ok(Some(expr::traversal([
                "data",
                "azurerm_client_config",
                self.client_config_label,
                "tenant_id",
            ]))),
            RuntimeEnvironmentValue::CurrentWorkerBindingName => {
                Ok(Some(Expression::String(self.worker_id.to_string())))
            }
            RuntimeEnvironmentValue::AwsAccountId
            | RuntimeEnvironmentValue::AwsRegion
            | RuntimeEnvironmentValue::BasePlatform
            | RuntimeEnvironmentValue::CurrentContainerBindingName
            | RuntimeEnvironmentValue::GcpProjectId
            | RuntimeEnvironmentValue::GcpRegion => Ok(None),
        }
    }

    fn render_runtime_environment_binding(
        &self,
        entry: &RuntimeEnvironmentBindingEntry,
    ) -> alien_core::Result<Option<Self::Value>> {
        render_linked_binding(self.ctx, self.registry, entry)
    }
}
