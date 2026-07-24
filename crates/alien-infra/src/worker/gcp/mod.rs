use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::worker::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_gcp_clients::cloudscheduler::{HttpTarget, SchedulerJob, SchedulerOidcToken};
use alien_gcp_clients::compute::{
    Address, AddressType, Backend, BackendService, BackendServiceProtocol, BalancingMode,
    ForwardingRule, ForwardingRuleProtocol, LoadBalancingScheme, NetworkEndpointGroup,
    NetworkEndpointGroupCloudRun, NetworkEndpointType, Operation as ComputeOperation,
    SslCertificate, SslCertificateSelfManaged, TargetHttpsProxy, UrlMap,
};
use alien_gcp_clients::longrunning::OperationResult;
use alien_gcp_clients::pubsub::{OidcToken, PushConfig, Subscription, Topic};
// Note: Role controller removed - workers now use ServiceAccount and permission profiles
use alien_core::{
    CertificateStatus, DnsRecordStatus, ResourceDefinition, ResourceOutputs, ResourceRef,
    ResourceStatus, Worker, WorkerOutputs,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;

mod create_dependencies;
mod create_load_balancer;
mod create_service;
mod delete;
mod helpers;
mod operations;
mod pubsub_helpers;
mod storage_helpers;
mod support;
#[cfg(test)]
mod tests;
mod update_dependencies;
mod update_load_balancer;
mod update_service;

use support::*;

pub use support::GcsNotificationTracker;

#[controller]
pub struct GcpWorkerController {
    /// The Cloud Run service name
    pub(crate) service_name: Option<String>,
    /// The invocation URL of the worker, available after creation.
    pub(crate) url: Option<String>,
    /// The operation name for long-running operations (for create, update, delete)
    pub(crate) operation_name: Option<String>,
    /// Number of targeted retries after GAR IAM propagation denied an image pull.
    #[serde(default)]
    pub(crate) image_pull_permission_retries: u8,
    /// The Compute Engine operation name for load-balancer infrastructure.
    pub(crate) compute_operation_name: Option<String>,
    /// Region for regional Compute Engine operations. `None` means global.
    pub(crate) compute_operation_region: Option<String>,
    /// Push subscription names for queue triggers (one per queue trigger)
    pub(crate) push_subscriptions: Vec<String>,
    /// Pub/Sub topic names created for storage trigger notifications
    pub(crate) storage_notification_topics: Vec<String>,
    /// GCS notification IDs for storage triggers (for cleanup)
    pub(crate) gcs_notification_ids: Vec<GcsNotificationTracker>,
    /// Cloud Scheduler job names for schedule triggers
    pub(crate) scheduler_job_names: Vec<String>,

    // Domain & Certificate
    /// The fully qualified domain name for the worker
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The GCP SSL certificate name
    pub(crate) ssl_certificate_name: Option<String>,
    /// Whether this worker uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // HTTPS Load Balancer components
    /// The serverless NEG name pointing to Cloud Run
    pub(crate) serverless_neg_name: Option<String>,
    /// The backend service name
    pub(crate) backend_service_name: Option<String>,
    /// The URL map name
    pub(crate) url_map_name: Option<String>,
    /// The target HTTPS proxy name
    pub(crate) target_https_proxy_name: Option<String>,
    /// The global static IP address name
    pub(crate) global_address_name: Option<String>,
    /// The global static IP address value
    pub(crate) global_address_ip: Option<String>,
    /// The forwarding rule name
    pub(crate) forwarding_rule_name: Option<String>,

    // GCP project/region (stored for binding output)
    /// The GCP project ID
    pub(crate) project_id: Option<String>,
    /// The GCP region
    pub(crate) region: Option<String>,

    // Commands infrastructure
    /// Pub/Sub topic short name for commands delivery (without project prefix)
    pub(crate) commands_topic_name: Option<String>,
    /// Pub/Sub subscription name for commands delivery
    pub(crate) commands_subscription_name: Option<String>,
}

#[controller]
impl GcpWorkerController {
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
        state = CreatingService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_service_impl(ctx).await
    }

    #[handler(
        state = RetryingImagePull,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn retrying_image_pull(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.retrying_image_pull_impl(ctx).await
    }

    #[handler(
        state = WaitingForServiceCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_service_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_service_creation_impl(ctx).await
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
        state = ImportingSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.importing_ssl_certificate_impl(ctx).await
    }

    #[handler(
        state = WaitingForSslCertificate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_ssl_certificate_impl(ctx).await
    }

    #[handler(
        state = CreatingServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_serverless_neg_impl(ctx).await
    }

    #[handler(
        state = WaitingForServerlessNeg,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_serverless_neg_impl(ctx).await
    }

    #[handler(
        state = CreatingBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_backend_service_impl(ctx).await
    }

    #[handler(
        state = WaitingForBackendService,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_backend_service_impl(ctx).await
    }

    #[handler(
        state = CreatingUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_url_map_impl(ctx).await
    }

    #[handler(
        state = WaitingForUrlMap,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_url_map_impl(ctx).await
    }

    #[handler(
        state = CreatingTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_target_https_proxy_impl(ctx).await
    }

    #[handler(
        state = WaitingForTargetHttpsProxy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_target_https_proxy_impl(ctx).await
    }

    #[handler(
        state = CreatingGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_global_address_impl(ctx).await
    }

    #[handler(
        state = WaitingForGlobalAddress,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_global_address_impl(ctx).await
    }

    #[handler(
        state = CreatingForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_forwarding_rule_impl(ctx).await
    }

    #[handler(
        state = WaitingForForwardingRule,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_forwarding_rule_impl(ctx).await
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
        state = CreatingPushSubscriptions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_push_subscriptions_impl(ctx).await
    }

    #[handler(
        state = CreatingSchedulerJobs,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_scheduler_jobs_impl(ctx).await
    }

    #[handler(
        state = CreatingCommandsInfrastructure,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_commands_infrastructure_impl(ctx).await
    }

    #[handler(
        state = SettingIamPolicy,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn setting_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.setting_iam_policy_impl(ctx).await
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
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.ready_impl(ctx).await
    }

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateImportingSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_importing_ssl_certificate_impl(ctx).await
    }

    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.update_start_impl(ctx).await
    }

    #[handler(
        state = UpdatingService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.updating_service_impl(ctx).await
    }

    #[handler(
        state = WaitingForServiceUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_service_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_service_update_impl(ctx).await
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
        state = UpdateImportingInitialSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_importing_initial_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_importing_initial_ssl_certificate_impl(ctx)
            .await
    }

    #[handler(
        state = UpdateWaitingForSslCertificate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_ssl_certificate_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_serverless_neg_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForServerlessNeg,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_serverless_neg_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_backend_service_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForBackendService,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_backend_service_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_url_map_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForUrlMap,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_url_map_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_target_https_proxy_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForTargetHttpsProxy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_target_https_proxy_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_global_address_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForGlobalAddress,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_global_address_impl(ctx).await
    }

    #[handler(
        state = UpdateCreatingForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_creating_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_creating_forwarding_rule_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForForwardingRule,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_forwarding_rule_impl(ctx).await
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
        state = UpdatePushSubscriptions,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_push_subscriptions_impl(ctx).await
    }

    #[handler(
        state = UpdateSettingIamPolicy,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_setting_iam_policy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_setting_iam_policy_impl(ctx).await
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

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.delete_start_impl(ctx).await
    }

    #[handler(
        state = DeletingForwardingRule,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_forwarding_rule(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_forwarding_rule_impl(ctx).await
    }

    #[handler(
        state = DeletingTargetHttpsProxy,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_target_https_proxy(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_target_https_proxy_impl(ctx).await
    }

    #[handler(
        state = DeletingUrlMap,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_url_map(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_url_map_impl(ctx).await
    }

    #[handler(
        state = DeletingBackendService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_backend_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_backend_service_impl(ctx).await
    }

    #[handler(
        state = DeletingServerlessNeg,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_serverless_neg(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_serverless_neg_impl(ctx).await
    }

    #[handler(
        state = DeletingSslCertificate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_ssl_certificate(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_ssl_certificate_impl(ctx).await
    }

    #[handler(
        state = DeletingGlobalAddress,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_global_address(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_global_address_impl(ctx).await
    }

    #[handler(
        state = DeletingPushSubscriptions,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_push_subscriptions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_push_subscriptions_impl(ctx).await
    }

    #[handler(
        state = DeletingSchedulerJobs,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_scheduler_jobs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_scheduler_jobs_impl(ctx).await
    }

    #[handler(
        state = DeletingCommandsInfrastructure,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_commands_infrastructure_impl(ctx).await
    }

    #[handler(
        state = DeletingService,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_service(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_service_impl(ctx).await
    }

    #[handler(
        state = WaitingForDeleteOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_delete_operation_impl(ctx).await
    }

    #[handler(
        state = WaitingForServiceDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_service_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_service_deletion_impl(ctx).await
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
        self.url.as_ref().map(|url| {
            let public_url = self
                .fqdn
                .as_ref()
                .map(|fqdn| format!("https://{fqdn}"))
                .unwrap_or_else(|| url.clone());

            let load_balancer_endpoint = self.global_address_ip.as_ref().map(|global_address_ip| {
                alien_core::LoadBalancerEndpoint {
                    dns_name: global_address_ip.clone(),
                    hosted_zone_id: None,
                }
            });

            ResourceOutputs::new(WorkerOutputs {
                // Use the service name if available, otherwise fall back to a placeholder
                worker_name: self
                    .service_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                identifier: self.service_name.clone(),
                public_endpoints: std::collections::HashMap::from([(
                    "default".to_string(),
                    alien_core::PublicEndpointOutput {
                        host: alien_core::public_url_host(&public_url).unwrap_or_default(),
                        protocol: alien_core::ExposeProtocol::Http,
                        port: alien_core::public_url_port(&public_url).unwrap_or(443),
                        url: public_url,
                        wildcard_host: None,
                        load_balancer_endpoint,
                    },
                )]),
                commands_push_target: self.commands_topic_name.clone(),
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, CloudRunWorkerBinding, WorkerBinding};

        if let (Some(service_name), Some(url)) = (&self.service_name, &self.url) {
            let project_id = self.project_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP project_id missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;
            let location = self.region.clone().ok_or_else(|| {
                AlienError::new(ErrorData::InfrastructureError {
                    message: "GCP region missing when building binding params".to_string(),
                    operation: Some("build_binding_params".to_string()),
                    resource_id: None,
                })
            })?;

            let binding = WorkerBinding::CloudRun(CloudRunWorkerBinding {
                project_id: BindingValue::Value(project_id),
                service_name: BindingValue::Value(service_name.clone()),
                location: BindingValue::Value(location),
                private_url: BindingValue::Value(url.clone()),
                public_url: Some(BindingValue::Value(url.clone())),
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
