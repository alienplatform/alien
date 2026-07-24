use alien_azure_clients::azure::resource_graph::ResourceGraphQueryRequest;
use alien_azure_clients::{
    AzureResourceGraphClient, AzureServiceBusManagementClient, AzureTokenCache, ResourceGraphApi,
    ServiceBusManagementApi,
};
use alien_core::import::{
    AzureRemoteStackManagementImportData, AzureServiceBusNamespaceImportData,
    GcpRemoteStackManagementImportData,
};
use alien_core::{
    AzureClientConfig, AzureCredentials, GcpClientConfig, GcpCredentials, GcpImpersonationConfig,
};
use alien_gcp_clients::{GcpClientConfigExt, ResourceManagerApi};
use tracing::warn;

use super::*;

/// Terraform can finish before GCP IAM bindings are visible.
/// Probe the same two-hop chain the manager uses before deployment starts:
/// management credentials -> configured manager SA -> imported stack management SA.
pub(super) async fn wait_for_gcp_management_permissions(
    config: &TestConfig,
    outputs: &Value,
    has_remote_management: bool,
) -> anyhow::Result<()> {
    if !has_remote_management {
        info!(
            "Skipping GCP management permission probe because Terraform rendered no remote management resource"
        );
        return Ok(());
    }

    let management_config: Option<ManagementConfig> = serde_json::from_str(
        &terraform_output_string(outputs, "deployment_management_config")?,
    )?;
    let Some(management_config) = management_config else {
        info!(
            "Skipping GCP management permission probe because Terraform output has no management config"
        );
        return Ok(());
    };
    let management_service_account_email = match management_config {
        ManagementConfig::Gcp(config) => config.service_account_email,
        other => {
            anyhow::bail!("expected GCP management config, got {other:?}");
        }
    };
    let target = config.gcp_target.as_ref().context("GCP target missing")?;
    let management_source = config.gcp_mgmt.as_ref();
    if management_service_account_email.is_empty() {
        warn!(
            "Skipping GCP management permission probe because no management service account is configured"
        );
        return Ok(());
    }
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(outputs, "deployment_resources")?)?;
    let remote_management = terraform_import_data::<GcpRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;

    let Some(management_source) = management_source else {
        warn!("Skipping GCP management permission probe because GCP management config is missing");
        return Ok(());
    };
    let Some(credentials_json) = management_source.credentials_json.clone() else {
        warn!(
            "Skipping GCP management permission probe because GCP management credentials are missing"
        );
        return Ok(());
    };

    let base_config = GcpClientConfig {
        project_id: management_source.project_id.clone(),
        region: management_source.region.clone(),
        credentials: GcpCredentials::ServiceAccountKey {
            json: credentials_json,
        },
        service_overrides: None,
        project_number: None,
    };
    let http = reqwest::Client::new();

    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let management_config = match base_config
            .impersonate(GcpImpersonationConfig {
                service_account_email: management_service_account_email.clone(),
                target_project_id: Some(target.project_id.clone()),
                target_region: Some(target.region.clone()),
                ..GcpImpersonationConfig::default()
            })
            .await
        {
            Ok(config) => config,
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP management service account impersonation did not propagate for {management_service_account_email} within {timeout:?}: {error}"
                    );
                }
                warn!(
                    service_account_email = %management_service_account_email,
                    attempt,
                    %error,
                    "GCP management service account impersonation is not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(error) => {
                anyhow::bail!("GCP management service account impersonation probe failed: {error}");
            }
        };
        let impersonated_config = match management_config
            .impersonate(GcpImpersonationConfig {
                service_account_email: remote_management.service_account_email.clone(),
                target_project_id: Some(target.project_id.clone()),
                target_region: Some(target.region.clone()),
                ..GcpImpersonationConfig::default()
            })
            .await
        {
            Ok(config) => config,
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP remote stack management service account impersonation did not propagate for {} within {timeout:?}: {error}",
                        remote_management.service_account_email
                    );
                }
                warn!(
                    service_account_email = %remote_management.service_account_email,
                    management_service_account_email = %management_service_account_email,
                    attempt,
                    %error,
                    "GCP remote stack management service account impersonation is not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
            Err(error) => {
                anyhow::bail!(
                    "GCP remote stack management service account impersonation probe failed: {error}"
                );
            }
        };

        let resource_manager = alien_gcp_clients::ResourceManagerClient::new(
            http.clone(),
            impersonated_config.clone(),
        );
        let result = resource_manager
            .get_project_metadata(target.project_id.clone())
            .await;

        match result {
            Ok(_) => {
                info!(
                    service_account_email = %remote_management.service_account_email,
                    attempts = attempt,
                    "GCP management IAM permissions are ready"
                );
                return Ok(());
            }
            Err(error) if gcp_management_permission_probe_should_retry(&error) => {
                if started.elapsed() >= timeout {
                    anyhow::bail!(
                        "GCP management IAM permissions did not propagate for {} within {timeout:?}: {error}",
                        remote_management.service_account_email
                    );
                }
                warn!(
                    service_account_email = %remote_management.service_account_email,
                    attempt,
                    %error,
                    "GCP management IAM permissions are not ready yet"
                );
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Err(error) => {
                anyhow::bail!("GCP management IAM permission probe failed: {error}");
            }
        }
    }
}

fn gcp_management_permission_probe_should_retry(error: &alien_gcp_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "REMOTE_ACCESS_DENIED" | "RATE_LIMIT_EXCEEDED" | "REMOTE_SERVICE_UNAVAILABLE" | "TIMEOUT"
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AzureManagementPermissionProbe {
    ServiceBus(AzureServiceBusNamespaceImportData),
    ResourceGraph,
}

pub(super) fn azure_management_permission_probe(
    resources: &[ImportedResource],
) -> anyhow::Result<AzureManagementPermissionProbe> {
    if let Some(service_bus) = optional_terraform_import_data::<AzureServiceBusNamespaceImportData>(
        resources,
        "azure_service_bus_namespace",
    )? {
        return Ok(AzureManagementPermissionProbe::ServiceBus(service_bus));
    }

    Ok(AzureManagementPermissionProbe::ResourceGraph)
}

/// Terraform can finish before Azure federated credentials and role
/// assignments are visible to ARM. When the stack has Service Bus management
/// permissions, exercise them directly. Otherwise query Resource Graph using
/// the baseline observe permission that every management profile receives.
pub(super) async fn wait_for_azure_management_permissions(
    config: &TestConfig,
    outputs: &Value,
) -> anyhow::Result<()> {
    let target = config
        .azure_target
        .as_ref()
        .context("Azure target missing")?;
    let resources: Vec<ImportedResource> =
        serde_json::from_str(&terraform_output_string(outputs, "deployment_resources")?)?;

    let management = terraform_import_data::<AzureRemoteStackManagementImportData>(
        &resources,
        "remote-stack-management",
    )?;
    let probe = azure_management_permission_probe(&resources)?;

    let token_file = std::env::var("AZURE_FEDERATED_TOKEN_FILE")
        .ok()
        .filter(|value| !value.is_empty())
        .context("AZURE_FEDERATED_TOKEN_FILE is required for Azure management permission probe")?;

    let azure_config = AzureClientConfig {
        subscription_id: management.subscription_id.clone(),
        tenant_id: management.tenant_id.clone(),
        region: Some(target.region.clone()),
        credentials: AzureCredentials::WorkloadIdentity {
            client_id: management.client_id.clone(),
            tenant_id: management.tenant_id.clone(),
            federated_token_file: token_file,
            authority_host: std::env::var("AZURE_AUTHORITY_HOST")
                .unwrap_or_else(|_| "https://login.microsoftonline.com/".to_string()),
        },
        service_overrides: None,
    };
    let timeout = Duration::from_secs(300);
    let started = tokio::time::Instant::now();
    let mut attempt = 0;
    match probe {
        AzureManagementPermissionProbe::ServiceBus(service_bus) => {
            let service_bus_client = AzureServiceBusManagementClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(azure_config),
            );
            let probe_queue_name = format!(
                "{}-iam-probe",
                terraform_output_string(outputs, "deployment_resource_prefix")?
            );

            loop {
                attempt += 1;
                let create_result = service_bus_client
                    .create_or_update_queue(
                        service_bus.resource_group.clone(),
                        service_bus.namespace_name.clone(),
                        probe_queue_name.clone(),
                        alien_azure_clients::models::queue::SbQueueProperties::default(),
                    )
                    .await;

                match create_result {
                    Ok(_) => {
                        match service_bus_client
                            .delete_queue(
                                service_bus.resource_group.clone(),
                                service_bus.namespace_name.clone(),
                                probe_queue_name.clone(),
                            )
                            .await
                        {
                            Ok(()) => {
                                info!(
                                    client_id = %management.client_id,
                                    attempts = attempt,
                                    "Azure management Service Bus permissions are ready"
                                );
                                return Ok(());
                            }
                            Err(error)
                                if azure_management_permission_probe_should_retry(&error) =>
                            {
                                if started.elapsed() >= timeout {
                                    anyhow::bail!(
                                        "Azure management IAM delete permissions did not propagate for {} within {timeout:?}: {error}",
                                        management.client_id
                                    );
                                }
                                warn!(
                                    client_id = %management.client_id,
                                    attempt,
                                    %error,
                                    "Azure management IAM delete permissions are not ready yet"
                                );
                            }
                            Err(error) => {
                                anyhow::bail!("Azure management IAM delete probe failed: {error}");
                            }
                        }
                    }
                    Err(error) if azure_management_permission_probe_should_retry(&error) => {
                        if started.elapsed() >= timeout {
                            anyhow::bail!(
                                "Azure management IAM permissions did not propagate for {} within {timeout:?}: {error}",
                                management.client_id
                            );
                        }
                        warn!(
                            client_id = %management.client_id,
                            attempt,
                            %error,
                            "Azure management IAM permissions are not ready yet"
                        );
                    }
                    Err(error) => {
                        anyhow::bail!("Azure management IAM permission probe failed: {error}");
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
        AzureManagementPermissionProbe::ResourceGraph => {
            let resource_graph_client = AzureResourceGraphClient::new(
                reqwest::Client::new(),
                AzureTokenCache::new(azure_config),
            );
            let request = ResourceGraphQueryRequest::for_subscription(
                management.subscription_id.clone(),
                "Resources | take 1 | project id",
            );

            loop {
                attempt += 1;
                match resource_graph_client.resources(request.clone()).await {
                    Ok(_) => {
                        info!(
                            client_id = %management.client_id,
                            attempts = attempt,
                            "Azure management Resource Graph permissions are ready"
                        );
                        return Ok(());
                    }
                    Err(error) if azure_management_permission_probe_should_retry(&error) => {
                        if started.elapsed() >= timeout {
                            anyhow::bail!(
                                "Azure management Resource Graph permissions did not propagate for {} within {timeout:?}: {error}",
                                management.client_id
                            );
                        }
                        warn!(
                            client_id = %management.client_id,
                            attempt,
                            %error,
                            "Azure management Resource Graph permissions are not ready yet"
                        );
                    }
                    Err(error) => {
                        anyhow::bail!(
                            "Azure management Resource Graph permission probe failed: {error}"
                        );
                    }
                }

                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

fn azure_management_permission_probe_should_retry(error: &alien_azure_clients::Error) -> bool {
    matches!(
        error.code.as_str(),
        "AUTHENTICATION_ERROR"
            | "AUTHENTICATION_FAILED"
            | "HTTP_RESPONSE_ERROR"
            | "REMOTE_ACCESS_DENIED"
            | "REMOTE_SERVICE_UNAVAILABLE"
            | "TIMEOUT"
            | "RATE_LIMIT_EXCEEDED"
    )
}
