//! # Azure Worker Controller Tests
//!
//! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

mod lro_routing_tests {
    use super::*;
    include!("azure_lro_routing_tests.rs");
}

mod commands_target_tests {
    use super::*;
    include!("azure_commands_target_tests.rs");
}

mod storage_target_tests {
    use super::*;
    include!("azure_storage_target_tests.rs");
    include!("azure_storage_delivery_update_tests.rs");
}

mod state_persistence_tests {
    use super::*;
    include!("azure_state_persistence_tests.rs");
}

mod operation_recovery_tests {
    use super::*;
    include!("azure_operation_recovery_tests.rs");
}

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use alien_azure_clients::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::models::container_apps::{
    Configuration, ConfigurationActiveRevisionsMode, ContainerApp, ContainerAppProperties,
    ContainerAppPropertiesProvisioningState,
};
use alien_azure_clients::{
    authorization::MockAuthorizationApi,
    container_apps::MockContainerAppsApi,
    event_grid::MockEventGridApi,
    long_running_operation::{LongRunningOperation, MockLongRunningOperationApi, OperationResult},
    service_bus::MockServiceBusManagementApi,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{Platform, ResourceStatus, Worker, WorkerOutputs, WorkerTrigger};
use alien_error::{AlienError, ContextError};
use httpmock::MockServer;
use rstest::rstest;

use super::{
    commands_queue_name, current_unix_timestamp_secs, dns_name_from_url,
    get_azure_internal_commands_dapr_component_name, AzureCommandsSenderRoleAssignmentIntent,
    AzureStorageTriggerTeardownProgress, AZURE_RBAC_WAIT_POLL_SECS,
};
use crate::core::{
    controller_test::{test_storage_1, SingleControllerExecutor},
    MockPlatformServiceProvider,
};
use crate::error::ErrorData;
use crate::infra_requirements::azure_utils::is_azure_authorization_propagation_error;
use crate::worker::azure::trigger_targets::azure_storage_event_types;
use crate::worker::azure_dapr_components::service_bus_dapr_component;
use crate::worker::azure_dapr_names_migration::CURRENT_DAPR_COMPONENT_NAMING_VERSION;
use crate::worker::{
    fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock, AzureWorkerController,
};
use crate::AzureWorkerState;

include!("azure_reconciliation_tests.rs");
include!("azure_test_support.rs");
include!("azure_lifecycle_tests.rs");
