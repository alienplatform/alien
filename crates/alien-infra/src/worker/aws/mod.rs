use std::{collections::HashMap, time::Duration};
use tracing::{debug, info, warn};

use crate::core::EnvironmentVariableBuilder;

use crate::core::split_certificate_chain;
use crate::core::ResourceController;
use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::readiness_probe::{
    run_readiness_probe_with_dns_override, READINESS_PROBE_MAX_ATTEMPTS,
};
use alien_aws_clients::apigatewayv2::{
    CreateApiMappingRequest, CreateApiRequest, CreateDomainNameRequest, CreateIntegrationRequest,
    CreateRouteRequest, CreateStageRequest, DomainNameConfiguration,
};
use alien_aws_clients::ec2::{DescribeNetworkInterfacesRequest, Filter};
use alien_aws_clients::eventbridge::{EventBridgeTarget, PutRuleRequest, PutTargetsRequest};
use alien_aws_clients::lambda::{
    AddPermissionRequest, CreateFunctionRequest, Environment, FunctionCode,
    ListEventSourceMappingsRequest, UpdateFunctionCodeRequest, UpdateFunctionConfigurationRequest,
    VpcConfig,
};
use alien_aws_clients::s3::{LambdaFunctionConfiguration, NotificationConfiguration};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    standard_resource_tags, CertificateStatus, DnsRecordStatus, Network, NetworkSettings,
    ResourceDefinition, ResourceOutputs, ResourceRef, ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

mod create_dependencies;
mod create_service;
mod delete;
mod helpers;
mod support;
#[cfg(test)]
mod tests;
mod update_dependencies;
mod update_exposure;
mod update_service;

use support::*;

pub use support::{LoadBalancerEndpoint, LoadBalancerState};

#[controller]
pub struct AwsWorkerController {
    pub(crate) arn: Option<String>,
    pub(crate) url: Option<String>,
    /// The logical AWS Lambda worker name (stack prefix + id). Stored to expose in outputs.
    pub(crate) worker_name: Option<String>,
    /// Event source mapping UUIDs for queue triggers
    pub(crate) event_source_mappings: Vec<String>,
    /// Fully qualified domain name for public ingress
    pub(crate) fqdn: Option<String>,
    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,
    /// ACM certificate ARN (auto-imported or custom)
    pub(crate) certificate_arn: Option<String>,
    /// API Gateway HTTP API ID
    pub(crate) api_id: Option<String>,
    /// API Gateway integration ID
    pub(crate) integration_id: Option<String>,
    /// API Gateway route ID
    pub(crate) route_id: Option<String>,
    /// API Gateway stage name
    pub(crate) stage_name: Option<String>,
    /// API Gateway API mapping ID
    pub(crate) api_mapping_id: Option<String>,
    /// API Gateway domain name
    pub(crate) domain_name: Option<String>,
    /// Endpoint metadata for DNS controller
    pub(crate) load_balancer: Option<LoadBalancerState>,
    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,
    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,
    /// Statement IDs for Lambda permissions granted to S3 for storage triggers
    pub(crate) s3_permission_statement_ids: Vec<String>,
    /// EventBridge rule names for schedule triggers
    pub(crate) eventbridge_rule_names: Vec<String>,
    /// Statement IDs for Lambda permissions granted to EventBridge for schedule triggers
    pub(crate) eventbridge_permission_statement_ids: Vec<String>,
}

#[controller]
impl AwsWorkerController {
    // ─────────────── CREATE FLOW ──────────────────────────────
    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.create_start_impl(ctx).await
    }

    #[handler(
        state = CreateWaitForActive,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.create_wait_for_active_impl(ctx).await
    }

    #[handler(
        state = WaitingForCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_certificate_impl(ctx).await
    }

    #[handler(
        state = ImportingCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.importing_certificate_impl(ctx).await
    }

    #[handler(
        state = CreatingApiGateway,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_gateway_impl(ctx).await
    }

    #[handler(
        state = CreatingApiIntegration,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_integration_impl(ctx).await
    }

    #[handler(
        state = CreatingApiRoute,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_route_impl(ctx).await
    }

    #[handler(
        state = CreatingApiStage,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_stage_impl(ctx).await
    }

    #[handler(
        state = CreatingApiDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_domain_impl(ctx).await
    }

    #[handler(
        state = CreatingApiMapping,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_api_mapping_impl(ctx).await
    }

    #[handler(
        state = AddingApiGatewayPermission,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.adding_api_gateway_permission_impl(ctx).await
    }

    #[handler(
        state = WaitingForDns,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dns_impl(ctx).await
    }

    #[handler(
        state = RunningReadinessProbe,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.running_readiness_probe_impl(ctx).await
    }

    #[handler(
        state = ApplyingResourcePermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.applying_resource_permissions_impl(ctx).await
    }

    #[handler(
        state = UpdatingEnvVarsWithSelfBinding,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn updating_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.updating_env_vars_with_self_binding_impl(ctx).await
    }

    #[handler(
        state = CreatingEventSourceMappings,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_event_source_mappings_impl(ctx).await
    }

    #[handler(
        state = CreatingScheduleTriggers,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_schedule_triggers_impl(ctx).await
    }

    #[handler(
        state = SettingConcurrency,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.setting_concurrency_impl(ctx).await
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.ready_impl(ctx).await
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_importing_certificate_impl(ctx).await
    }

    #[handler(
        state = UpdateCodeStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_code_start_impl(ctx).await
    }

    #[handler(
        state = UpdateCodeWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_code_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_code_wait_for_active_impl(ctx).await
    }

    #[handler(
        state = UpdateConfigStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_start(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_config_start_impl(ctx).await
    }

    #[handler(
        state = UpdateConfigWaitForActive,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_config_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_config_wait_for_active_impl(ctx).await
    }

    #[handler(
        state = UpdateEnsuringPublicExposure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_ensuring_public_exposure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_ensuring_public_exposure_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_certificate_impl(ctx).await
    }

    #[handler(
        state = UpdateImportingInitialCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_importing_initial_certificate_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiGateway,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_gateway_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiIntegration,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_integration(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_integration_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiRoute,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_route(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_route_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiStage,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_stage(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_stage_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiDomain,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_domain_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingApiMapping,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_api_mapping(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_api_mapping_impl(ctx).await
    }

    #[handler(
        state = UpdateAddingApiGatewayPermission,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_adding_api_gateway_permission(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_adding_api_gateway_permission_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForDns,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_dns(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_dns_impl(ctx).await
    }

    #[handler(
        state = UpdateRunningReadinessProbe,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_running_readiness_probe(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_running_readiness_probe_impl(ctx).await
    }

    #[handler(
        state = UpdateApplyingResourcePermissions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_applying_resource_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_applying_resource_permissions_impl(ctx).await
    }

    #[handler(
        state = UpdateEnvVarsWithSelfBinding,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_env_vars_with_self_binding(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_env_vars_with_self_binding_impl(ctx).await
    }

    #[handler(
        state = UpdateEventSourceMappings,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_event_source_mappings_impl(ctx).await
    }

    #[handler(
        state = UpdatingConcurrency,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_concurrency(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.updating_concurrency_impl(ctx).await
    }

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(
        &mut self,
        _ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.delete_start_impl(_ctx).await
    }

    #[handler(
        state = DeletingApiGateway,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_api_gateway(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_api_gateway_impl(ctx).await
    }

    #[handler(
        state = DeletingEventSourceMappings,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_event_source_mappings(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_event_source_mappings_impl(ctx).await
    }

    #[handler(
        state = DeletingScheduleTriggers,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_schedule_triggers(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_schedule_triggers_impl(ctx).await
    }

    #[handler(
        state = DetachingVpcConfig,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detaching_vpc_config(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.detaching_vpc_config_impl(ctx).await
    }

    #[handler(
        state = DetachVpcWaitForActive,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn detach_vpc_wait_for_active(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.detach_vpc_wait_for_active_impl(ctx).await
    }

    #[handler(
        state = DeletingWorker,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_function(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_function_impl(ctx).await
    }

    #[handler(
        state = DeleteWaitForNotFound,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_wait_for_not_found(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.delete_wait_for_not_found_impl(ctx).await
    }

    #[handler(
        state = WaitingForVpcEnisReleased,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_vpc_enis_released(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_vpc_enis_released_impl(ctx).await
    }

    #[handler(
        state = DeletingCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_certificate_impl(ctx).await
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );

    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);

    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);

    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.arn.as_ref().map(|arn| {
            // Map the load balancer endpoint for DNS management
            let load_balancer_endpoint = self
                .load_balancer
                .as_ref()
                .and_then(|lb| lb.endpoint.as_ref())
                .map(|endpoint| alien_core::LoadBalancerEndpoint {
                    dns_name: endpoint.dns_name.clone(),
                    hosted_zone_id: Some(endpoint.hosted_zone_id.clone()),
                });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .worker_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                identifier: Some(arn.clone()),
                public_endpoints: self
                    .url
                    .as_ref()
                    .map(|url| {
                        std::collections::HashMap::from([(
                            "default".to_string(),
                            alien_core::PublicEndpointOutput {
                                host: alien_core::public_url_host(url).unwrap_or_default(),
                                protocol: alien_core::ExposeProtocol::Http,
                                port: alien_core::public_url_port(url).unwrap_or(443),
                                url: url.clone(),
                                wildcard_host: None,
                                load_balancer_endpoint,
                            },
                        )])
                    })
                    .unwrap_or_default(),
                commands_push_target: self.worker_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, LambdaWorkerBinding, WorkerBinding};

        if let (Some(worker_name), Some(arn)) = (&self.worker_name, &self.arn) {
            // Extract region from ARN: arn:aws:lambda:us-east-1:123456789:function:name
            let region = arn.split(':').nth(3).unwrap_or("us-east-1").to_string();

            let binding = WorkerBinding::Lambda(LambdaWorkerBinding {
                worker_name: BindingValue::Value(worker_name.clone()),
                region: BindingValue::Value(region),
                url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
            });
            Ok(Some(
                serde_json::to_value(binding).into_alien_error().context(
                    ErrorData::ResourceStateSerializationFailed {
                        resource_id: "binding".to_string(),
                        message: "Failed to serialize binding parameters".to_string(),
                    },
                )?,
            ))
        } else {
            Ok(None)
        }
    }
}
