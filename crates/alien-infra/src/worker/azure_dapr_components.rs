use alien_azure_clients::container_apps::ContainerAppsApi;
use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::managed_environments_dapr_components::{
    DaprComponent, DaprComponentProperties, DaprMetadata, Secret,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_error::{AlienError, Context, ContextError};
use tracing::{info, warn};

use crate::error::{ErrorData, Result};

pub(super) enum DaprComponentDeleteOperation {
    NotFound,
    Foreign,
    Completed,
    LongRunning(LongRunningOperation),
}

#[derive(Debug)]
pub(super) enum DaprComponentEnsureOperation {
    Unchanged,
    Completed,
    LongRunning(LongRunningOperation),
}

pub(super) enum LegacyDaprComponentCleanupStep {
    Complete,
    Mutated,
    LongRunning(LongRunningOperation),
}

pub(super) enum DaprComponentOwnership {
    NotFound,
    Owned(DaprComponent),
    Foreign,
}

pub(super) enum TrackedDaprComponentDeleteStep {
    Complete,
    Mutated,
    LongRunning {
        operation: LongRunningOperation,
        component_name: String,
    },
}

pub(super) fn service_bus_dapr_component(
    component_name: String,
    container_app_name: &str,
    namespace_name: &str,
    queue_name: String,
    azure_client_id: &str,
) -> DaprComponent {
    let metadata = vec![
        DaprMetadata {
            name: Some("namespaceName".into()),
            value: Some(format!("{namespace_name}.servicebus.windows.net")),
            secret_ref: None,
        },
        DaprMetadata {
            name: Some("queueName".into()),
            value: Some(queue_name),
            secret_ref: None,
        },
        DaprMetadata {
            name: Some("direction".into()),
            value: Some("input".into()),
            secret_ref: None,
        },
        DaprMetadata {
            name: Some("azureClientId".into()),
            value: Some(azure_client_id.to_string()),
            secret_ref: None,
        },
    ];

    DaprComponent {
        name: Some(component_name),
        properties: Some(DaprComponentProperties {
            component_type: Some("bindings.azure.servicebusqueues".to_string()),
            ignore_errors: false,
            init_timeout: None,
            version: Some("v1".to_string()),
            metadata,
            scopes: vec![container_app_name.to_string()],
            secret_store_component: None,
            secrets: vec![],
        }),
        id: None,
        system_data: None,
        type_: None,
    }
}

pub(super) async fn get_dapr_component_ownership(
    client: &dyn ContainerAppsApi,
    resource_group_name: &str,
    environment_name: &str,
    container_app_name: &str,
    component_name: &str,
    worker_id: &str,
) -> Result<DaprComponentOwnership> {
    let component = match client
        .get_dapr_component(resource_group_name, environment_name, component_name)
        .await
    {
        Ok(component) => component,
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            return Ok(DaprComponentOwnership::NotFound);
        }
        Err(error) => {
            return Err(error.context(ErrorData::CloudPlatformError {
                message: format!("Failed to inspect Dapr component '{component_name}'"),
                resource_id: Some(worker_id.to_string()),
            }));
        }
    };

    let scopes = component
        .properties
        .as_ref()
        .map(|properties| properties.scopes.as_slice())
        .unwrap_or_default();
    if scopes == [container_app_name] {
        Ok(DaprComponentOwnership::Owned(component))
    } else {
        warn!(
            worker=%worker_id,
            component=%component_name,
            scopes=?scopes,
            "Dapr component is not exclusively scoped to this worker"
        );
        Ok(DaprComponentOwnership::Foreign)
    }
}

pub(super) async fn delete_dapr_component_if_owned(
    client: &dyn ContainerAppsApi,
    resource_group_name: &str,
    environment_name: &str,
    container_app_name: &str,
    component_name: &str,
    worker_id: &str,
) -> Result<DaprComponentDeleteOperation> {
    match get_dapr_component_ownership(
        client,
        resource_group_name,
        environment_name,
        container_app_name,
        component_name,
        worker_id,
    )
    .await?
    {
        DaprComponentOwnership::NotFound => return Ok(DaprComponentDeleteOperation::NotFound),
        DaprComponentOwnership::Foreign => return Ok(DaprComponentDeleteOperation::Foreign),
        DaprComponentOwnership::Owned(_) => {}
    }

    match client
        .delete_dapr_component(resource_group_name, environment_name, component_name)
        .await
    {
        Ok(OperationResult::Completed(())) => Ok(DaprComponentDeleteOperation::Completed),
        Ok(OperationResult::LongRunning(lro)) => Ok(DaprComponentDeleteOperation::LongRunning(lro)),
        Err(error)
            if matches!(
                error.error,
                Some(CloudClientErrorData::RemoteResourceNotFound { .. })
            ) =>
        {
            Ok(DaprComponentDeleteOperation::NotFound)
        }
        Err(error) => Err(error.context(ErrorData::CloudPlatformError {
            message: format!("Failed to delete Dapr component '{component_name}'"),
            resource_id: Some(worker_id.to_string()),
        })),
    }
}

async fn dapr_component_needs_write(
    client: &dyn ContainerAppsApi,
    resource_group_name: &str,
    environment_name: &str,
    container_app_name: &str,
    desired: &DaprComponent,
    worker_id: &str,
) -> Result<bool> {
    let component_name = desired.name.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: worker_id.to_string(),
            message: "Desired Dapr component has no name".to_string(),
        })
    })?;
    match get_dapr_component_ownership(
        client,
        resource_group_name,
        environment_name,
        container_app_name,
        component_name,
        worker_id,
    )
    .await?
    {
        DaprComponentOwnership::NotFound => Ok(true),
        DaprComponentOwnership::Owned(existing) => Ok(!dapr_component_matches(&existing, desired)),
        DaprComponentOwnership::Foreign => Err(AlienError::new(ErrorData::ResourceDrift {
            resource_id: worker_id.to_string(),
            message: format!("Dapr component '{component_name}' is owned by another Container App"),
        })),
    }
}

pub(super) async fn ensure_dapr_component(
    client: &dyn ContainerAppsApi,
    resource_group_name: &str,
    environment_name: &str,
    container_app_name: &str,
    desired: &DaprComponent,
    worker_id: &str,
) -> Result<DaprComponentEnsureOperation> {
    let component_name = desired.name.as_deref().ok_or_else(|| {
        AlienError::new(ErrorData::ResourceControllerConfigError {
            resource_id: worker_id.to_string(),
            message: "Desired Dapr component has no name".to_string(),
        })
    })?;
    if !dapr_component_needs_write(
        client,
        resource_group_name,
        environment_name,
        container_app_name,
        desired,
        worker_id,
    )
    .await?
    {
        return Ok(DaprComponentEnsureOperation::Unchanged);
    }

    match client
        .create_or_update_dapr_component(
            resource_group_name,
            environment_name,
            component_name,
            desired,
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to create or update Dapr component '{component_name}'"),
            resource_id: Some(worker_id.to_string()),
        })? {
        OperationResult::Completed(_) => Ok(DaprComponentEnsureOperation::Completed),
        OperationResult::LongRunning(operation) => {
            Ok(DaprComponentEnsureOperation::LongRunning(operation))
        }
    }
}

pub(super) async fn delete_owned_legacy_dapr_components(
    client: &dyn ContainerAppsApi,
    resource_group_name: &str,
    environment_name: &str,
    container_app_name: &str,
    desired_component_name: &str,
    legacy_component_names: &[String],
    worker_id: &str,
) -> Result<LegacyDaprComponentCleanupStep> {
    for legacy_component_name in legacy_component_names {
        if legacy_component_name == desired_component_name {
            continue;
        }

        match delete_dapr_component_if_owned(
            client,
            resource_group_name,
            environment_name,
            container_app_name,
            legacy_component_name,
            worker_id,
        )
        .await?
        {
            DaprComponentDeleteOperation::NotFound | DaprComponentDeleteOperation::Foreign => {}
            DaprComponentDeleteOperation::Completed => {
                return Ok(LegacyDaprComponentCleanupStep::Mutated);
            }
            DaprComponentDeleteOperation::LongRunning(lro) => {
                info!(
                    worker=%worker_id,
                    component=%legacy_component_name,
                    replacement=%desired_component_name,
                    "Waiting for legacy Dapr component deletion before creating its structured replacement"
                );
                return Ok(LegacyDaprComponentCleanupStep::LongRunning(lro));
            }
        }
    }

    Ok(LegacyDaprComponentCleanupStep::Complete)
}

pub(super) fn dapr_component_matches(existing: &DaprComponent, desired: &DaprComponent) -> bool {
    let (Some(existing_properties), Some(desired_properties)) =
        (existing.properties.as_ref(), desired.properties.as_ref())
    else {
        return false;
    };

    existing.name == desired.name
        && existing_properties.component_type == desired_properties.component_type
        && existing_properties.ignore_errors == desired_properties.ignore_errors
        && existing_properties.init_timeout == desired_properties.init_timeout
        && existing_properties.version == desired_properties.version
        && existing_properties.scopes == desired_properties.scopes
        && existing_properties.secret_store_component == desired_properties.secret_store_component
        && normalized_metadata(&existing_properties.metadata)
            == normalized_metadata(&desired_properties.metadata)
        && normalized_secrets(&existing_properties.secrets)
            == normalized_secrets(&desired_properties.secrets)
}

fn normalized_metadata(
    metadata: &[DaprMetadata],
) -> Vec<(Option<&str>, Option<&str>, Option<&str>)> {
    let mut normalized = metadata
        .iter()
        .map(|entry| {
            (
                entry.name.as_deref(),
                entry.value.as_deref(),
                entry.secret_ref.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized
}

fn normalized_secrets(
    secrets: &[Secret],
) -> Vec<(Option<&str>, Option<&str>, Option<&str>, Option<&str>)> {
    let mut normalized = secrets
        .iter()
        .map(|secret| {
            (
                secret.name.as_deref(),
                secret.value.as_deref(),
                secret.identity.as_deref(),
                secret.key_vault_url.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    normalized
}
