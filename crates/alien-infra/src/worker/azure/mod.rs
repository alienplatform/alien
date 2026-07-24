use alien_azure_clients::container_apps::{
    ManagedEnvironmentCertificate, ManagedEnvironmentCertificateKeyVaultProperties,
    ManagedEnvironmentCertificateProperties,
};
use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::container_apps::{
    Configuration, ConfigurationActiveRevisionsMode, Container, ContainerApp,
    ContainerAppProperties, ContainerAppPropertiesProvisioningState, ContainerResources,
    CustomDomain, CustomDomainBindingType, EnvironmentVar, IdentitySettings,
    IdentitySettingsLifecycle, IngressTransport, RegistryCredentials, Scale, Secret, Template,
    TrafficWeight,
};
use alien_azure_clients::AzureClientConfig;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    AzureContainerAppsWorkerHeartbeatData, CertificateStatus, DnsRecordStatus, HeartbeatBackend,
    ObservedHealth, Platform, ProviderLifecycleState, RemoteStackManagement,
    RemoteStackManagementOutputs, ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs,
    ResourceRef, ResourceStatus, Worker, WorkerHeartbeatData, WorkerOutputs,
    WorkloadHeartbeatStatus, ENV_AZURE_CLIENT_ID,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use base64::Engine;
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::core::EnvironmentVariableBuilder;
use crate::core::{ResourceController, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::infra_requirements::azure_utils;
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_name, get_container_apps_environment_outputs,
    get_resource_group_name, is_azure_authorization_propagation_error,
};
use crate::worker::azure_dapr_components::{
    delete_owned_legacy_dapr_components, ensure_dapr_component, service_bus_dapr_component,
    DaprComponentEnsureOperation, LegacyDaprComponentCleanupStep, TrackedDaprComponentDeleteStep,
};
use crate::worker::azure_dapr_names_migration::{
    DaprComponentMigrationStep, CURRENT_DAPR_COMPONENT_NAMING_VERSION,
};
use crate::worker::azure_names::{
    commands_queue_name, get_azure_blob_trigger_dapr_component_name, get_azure_container_app_name,
    get_azure_dapr_component_name, get_azure_internal_commands_dapr_component_name,
    get_azure_queue_trigger_dapr_component_name, get_azure_storage_event_subscription_name,
    get_legacy_azure_blob_trigger_dapr_component_names,
    get_legacy_azure_internal_commands_dapr_component_names,
    get_legacy_azure_queue_trigger_dapr_component_names,
};
use crate::worker::readiness_probe::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use alien_macros::controller;

mod create_app;
mod create_dependencies;
mod delete;
mod update_dependencies;
mod update_prepare;

#[path = "../azure_cleanup.rs"]
mod cleanup;
use cleanup::{AzureCommandsQueueTarget, CommandsQueueTargetPreparation};
#[path = "../azure_command_sender.rs"]
mod command_sender;
use command_sender::{AzureCommandsSenderRoleAssignmentIntent, CommandsSenderReconcileResult};
#[path = "../azure_operations.rs"]
mod operations;
use operations::{
    poll_pending_operation, poll_reconciled_operation, AzureOperationPoll,
    AzureOperationPollRequest, AzureStrictOperationPoll,
};
#[path = "../azure_role_assignments.rs"]
mod role_assignments;
#[path = "../azure_trigger_targets.rs"]
mod trigger_targets;
use trigger_targets::{StorageDeliveryReconcileResult, StorageTargetPreparation};

mod helpers;
mod support;
mod trigger_helpers;

use support::*;

pub use support::AzureStorageTriggerInfrastructure;

// ≡ Controller definition =======================================================
#[controller]
pub struct AzureWorkerController {
    // ─────────── Persisted fields ───────────
    /// Azure Container App name. Filled on *create* and reused for update/delete.
    pub(crate) container_app_name: Option<String>,

    /// Resource ID of the Container App (ARM ID).
    pub(crate) resource_id: Option<String>,

    /// Public URL (if `Ingress::Public`).
    pub(crate) url: Option<String>,

    /// The Container App's own ingress host (`*.azurecontainerapps.io`). `url` may be overridden to
    /// the public display FQDN (from `public_urls`), but DNS records must target THIS host:
    /// targeting the public FQDN makes the CNAME self-referential (target == record name) and the
    /// DNS provider rejects it as a loop. See `build_outputs`.
    pub(crate) container_app_url: Option<String>,

    /// URL returned by Azure ARM for *current* long‑running operation.
    pub(crate) pending_operation_url: Option<String>,
    /// Retry‑after seconds for the current LRO (populated when Azure returns it).
    pub(crate) pending_operation_retry_after: Option<u64>,
    /// Dapr component names for all worker triggers.
    pub(crate) dapr_components: Vec<String>,
    /// Event Grid and Service Bus resources created for storage triggers.
    #[serde(default)]
    pub(crate) storage_trigger_infrastructure: Vec<AzureStorageTriggerInfrastructure>,
    /// Next durable resource deletion within the first tracked storage trigger.
    #[serde(default)]
    pub(crate) storage_trigger_teardown_progress: AzureStorageTriggerTeardownProgress,

    // Domain & Certificate
    /// The fully qualified domain name for the worker
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The Azure Key Vault certificate ID
    pub(crate) keyvault_cert_id: Option<String>,
    /// The Azure Container Apps managed environment certificate resource ID.
    pub(crate) container_apps_certificate_id: Option<String>,
    /// Whether this worker uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // Commands infrastructure
    /// Service Bus resource group used for commands delivery.
    #[serde(default)]
    pub(crate) commands_resource_group_name: Option<String>,
    /// Service Bus namespace name for commands delivery
    pub(crate) commands_namespace_name: Option<String>,
    /// Service Bus queue name for commands delivery
    pub(crate) commands_queue_name: Option<String>,
    /// Whether the tracked commands queue has been applied in the current setup cycle.
    #[serde(default)]
    pub(crate) commands_queue_applied: bool,
    /// Dapr component name for commands queue
    pub(crate) commands_dapr_component: Option<String>,
    /// Current and historical command Dapr names still requiring ownership-aware teardown.
    #[serde(default)]
    pub(crate) commands_dapr_component_deletion_candidates: Vec<String>,
    /// Role assignment ID for Service Bus Data Sender on the deploying identity (for cleanup)
    pub(crate) commands_sender_role_assignment_id: Option<String>,
    /// Durable direct-manager sender grant planned before its idempotent Azure PUT.
    #[serde(default)]
    pub(crate) commands_sender_role_assignment_intent:
        Option<AzureCommandsSenderRoleAssignmentIntent>,
    /// Whether the exact commands queue has been inspected for controller-owned sender grants.
    #[serde(default)]
    pub(crate) commands_sender_role_assignment_discovery_complete: bool,
    /// Legacy setup-owned receiver cursor. It is ignored and never remotely deleted.
    pub(crate) commands_receiver_role_assignment_id: Option<String>,

    /// Deadline for retrying commands infrastructure creation while Azure IAM grants propagate.
    #[serde(default)]
    pub(crate) commands_infrastructure_auth_wait_until_epoch_secs: Option<u64>,
    /// Deadline for retrying Container Apps Environment operations while Azure wakes an idle environment.
    #[serde(default)]
    pub(crate) container_apps_environment_wake_wait_until_epoch_secs: Option<u64>,
    /// Next time the controller should retry a Container Apps Environment operation after an idle wake response.
    #[serde(default)]
    pub(crate) container_apps_environment_wake_retry_after_epoch_secs: Option<u64>,
    /// Deadline before creating the Container App after pre-created RBAC assignments.
    #[serde(default)]
    pub(crate) pre_container_app_rbac_wait_until_epoch_secs: Option<u64>,
    /// Deadline before reporting Ready after all consumer-visible permissions were applied.
    #[serde(default)]
    pub(crate) ready_rbac_wait_until_epoch_secs: Option<u64>,
    /// Whether the current update flow changed the workload and should wait for RBAC propagation.
    #[serde(default)]
    pub(crate) update_rbac_wait_required: bool,
    /// Whether the current update flow has already deleted old Dapr trigger components.
    #[serde(default)]
    pub(crate) update_dapr_components_deleted: bool,
    /// Version of the deterministic Dapr component naming scheme applied to this worker.
    #[serde(default)]
    pub(crate) dapr_component_naming_version: u8,
    /// Trigger component whose asynchronous deletion is currently being polled.
    #[serde(default)]
    pub(crate) pending_dapr_component_deletion_name: Option<String>,
    /// Whether delete has persisted the complete current and historical Dapr cleanup plan.
    #[serde(default)]
    pub(crate) dapr_component_deletion_candidates_initialized: bool,
    /// Whether imported auxiliary command/storage cleanup candidates have been reconstructed.
    #[serde(default)]
    pub(crate) auxiliary_teardown_candidates_initialized: bool,
    /// Whether a commands-only update teardown has reconstructed imported cleanup cursors.
    #[serde(default)]
    pub(crate) commands_update_teardown_candidates_initialized: bool,
    /// Whether trigger update teardown has reconstructed candidates from the previous config.
    #[serde(default)]
    pub(crate) trigger_update_teardown_candidates_initialized: bool,
    /// Whether this update durably invalidated storage delivery verification latches.
    #[serde(default)]
    pub(crate) storage_delivery_update_reconciliation_initialized: bool,
}

#[controller]
impl AzureWorkerController {
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
        state = WaitingForPreCreateCommandsDaprComponentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_pre_create_commands_dapr_component_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_pre_create_commands_dapr_component_operation_impl(ctx)
            .await
    }

    #[handler(
        state = WaitingForPreCreateDaprComponentDeletion,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_pre_create_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_pre_create_dapr_component_deletion_impl(ctx)
            .await
    }

    #[handler(
        state = WaitingBeforeContainerAppCreation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_before_container_app_creation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_before_container_app_creation_impl(ctx).await
    }

    #[handler(
        state = CreatingContainerAppResource,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_container_app_resource(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_container_app_resource_impl(ctx).await
    }

    #[handler(
        state = WaitingForCreateOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_create_operation_impl(ctx).await
    }

    #[handler(
        state = CreatingContainerApp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.creating_container_app_impl(ctx).await
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
        state = ConfiguringCustomDomain,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_custom_domain(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.configuring_custom_domain_impl(ctx).await
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
        state = ConfiguringDaprComponents,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn configuring_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.configuring_dapr_components_impl(ctx).await
    }

    #[handler(
        state = WaitingForDaprComponentCreateOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_dapr_component_create_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dapr_component_create_operation_impl(ctx)
            .await
    }

    #[handler(
        state = WaitingForLegacyDaprComponentDeletionDuringCreate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_legacy_dapr_component_deletion_during_create(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_legacy_dapr_component_deletion_during_create_impl(ctx)
            .await
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
        state = WaitingForCommandsDaprComponentOperation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_commands_dapr_component_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_commands_dapr_component_operation_impl(ctx)
            .await
    }

    #[handler(
        state = WaitingForLegacyCommandsDaprComponentDeletionDuringCreate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_legacy_commands_dapr_component_deletion_during_create(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_legacy_commands_dapr_component_deletion_during_create_impl(ctx)
            .await
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
        state = ApplyingPermissions,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn applying_permissions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.applying_permissions_impl(ctx).await
    }

    #[handler(
        state = WaitingForRbacPropagation,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_rbac_propagation_impl(ctx).await
    }

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.ready_impl(ctx).await
    }

    #[handler(
        state = MigratingDaprComponentNames,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn migrating_dapr_component_names(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.migrating_dapr_component_names_impl(ctx).await
    }

    #[handler(
        state = WaitingForDaprComponentNameMigrationOperation,
        on_failure = RefreshFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_name_migration_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dapr_component_name_migration_operation_impl(ctx)
            .await
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
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.update_start_impl(ctx).await
    }

    #[handler(
        state = WaitingForUpdateOperation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_update_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_update_operation_impl(ctx).await
    }

    #[handler(
        state = UpdatingContainerApp,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.updating_container_app_impl(ctx).await
    }

    #[handler(
        state = UpdateDaprComponents,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_dapr_components_impl(ctx).await
    }

    #[handler(
        state = WaitingForDaprComponentDeletionForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_deletion_for_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dapr_component_deletion_for_update_impl(ctx)
            .await
    }

    #[handler(
        state = UpdateWaitingForLegacyDaprComponentDeletion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_legacy_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_legacy_dapr_component_deletion_impl(ctx)
            .await
    }

    #[handler(
        state = WaitingForDaprComponentUpdateOperation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_dapr_component_update_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dapr_component_update_operation_impl(ctx)
            .await
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
        state = UpdateWaitingForRbacPropagation,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_rbac_propagation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_rbac_propagation_impl(ctx).await
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
        state = WaitingForPendingOperationBeforeDelete,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_pending_operation_before_delete(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_pending_operation_before_delete_impl(ctx)
            .await
    }

    #[handler(
        state = DeletingDaprComponents,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_dapr_components_impl(ctx).await
    }

    #[handler(
        state = WaitingForDaprComponentDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_dapr_component_deletion_impl(ctx).await
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
        state = WaitingForCommandsDaprComponentDeletion,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_commands_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_commands_dapr_component_deletion_impl(ctx)
            .await
    }

    #[handler(
        state = UpdateDeletingCommandsInfrastructure,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_deleting_commands_infrastructure(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_deleting_commands_infrastructure_impl(ctx).await
    }

    #[handler(
        state = UpdateWaitingForCommandsDaprComponentDeletion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_commands_dapr_component_deletion(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_commands_dapr_component_deletion_impl(ctx)
            .await
    }

    #[handler(
        state = UpdateWaitingForCommandsDaprComponentDeletionForSetup,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_waiting_for_commands_dapr_component_deletion_for_setup(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.update_waiting_for_commands_dapr_component_deletion_for_setup_impl(ctx)
            .await
    }

    #[handler(
        state = DeletingApp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_app(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        self.deleting_app_impl(ctx).await
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
        state = DeletingContainerApp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_container_app(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.deleting_container_app_impl(ctx).await
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

    #[handler(
        state = WaitingForCertificateDeleteOperation,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn waiting_for_certificate_delete_operation(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.waiting_for_certificate_delete_operation_impl(ctx)
            .await
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    // Implementation of get_outputs trait method
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.resource_id.as_ref().map(|id| {
            // CNAME target = the ingress host; fall back to `url` when `container_app_url` is unset.
            let load_balancer_endpoint =
                self.container_app_url
                    .as_ref()
                    .or(self.url.as_ref())
                    .map(|host| alien_core::LoadBalancerEndpoint {
                        dns_name: dns_name_from_url(host),
                        hosted_zone_id: None,
                    });

            ResourceOutputs::new(WorkerOutputs {
                worker_name: self
                    .container_app_name
                    .clone()
                    .unwrap_or_else(|| "worker-name-placeholder".to_string()),
                identifier: Some(id.clone()),
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
                commands_push_target: match (
                    &self.commands_namespace_name,
                    &self.commands_queue_name,
                ) {
                    (Some(ns), Some(q)) => Some(format!("{}/{}", ns, q)),
                    _ => None,
                },
            })
        })
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        use alien_core::bindings::{BindingValue, ContainerAppWorkerBinding, WorkerBinding};

        if let (Some(container_app_name), Some(resource_id)) =
            (&self.container_app_name, &self.resource_id)
        {
            // Extract resource group name from ARM resource ID
            // Format: /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.App/containerApps/{name}
            let resource_group_name = resource_id
                .split('/')
                .nth(4)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Malformed ARM resource ID (missing resource group): {}",
                            resource_id
                        ),
                        operation: Some("parse_arm_resource_id".to_string()),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?
                .to_string();

            // Extract subscription ID from ARM resource ID
            // Format: /subscriptions/{sub}/resourceGroups/{rg}/providers/...
            let subscription_id = resource_id
                .split('/')
                .nth(2)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Malformed ARM resource ID (missing subscription): {}",
                            resource_id
                        ),
                        operation: Some("parse_arm_resource_id".to_string()),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?
                .to_string();

            // Private URL is the internal FQDN (same as public URL for Container Apps
            // with external ingress; for internal ingress it would differ).
            let private_url = self
                .url
                .clone()
                .unwrap_or_else(|| format!("https://{}", container_app_name));

            let binding = WorkerBinding::ContainerApp(ContainerAppWorkerBinding {
                subscription_id: BindingValue::Value(subscription_id),
                resource_group_name: BindingValue::Value(resource_group_name),
                container_app_name: BindingValue::Value(container_app_name.clone()),
                private_url: BindingValue::Value(private_url),
                public_url: self.url.as_ref().map(|u| BindingValue::Value(u.clone())),
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

#[cfg(test)]
#[path = "../azure_tests.rs"]
mod tests;
