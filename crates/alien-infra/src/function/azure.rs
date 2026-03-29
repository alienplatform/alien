use alien_azure_clients::long_running_operation::{LongRunningOperation, OperationResult};
use alien_azure_clients::models::certificates::CertificateImportParameters;
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
    CertificateStatus, DnsRecordStatus, Function, FunctionOutputs, Ingress, ResourceOutputs,
    ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use base64::Engine;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::core::EnvironmentVariableBuilder;
use crate::core::{ResourceController, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::function::readiness_probe::{run_readiness_probe, READINESS_PROBE_MAX_ATTEMPTS};
use crate::infra_requirements::azure_utils;
use crate::infra_requirements::azure_utils::{
    get_container_apps_environment_name, get_resource_group_name,
};
use alien_macros::controller;

/// Generates a deterministic Azure Container Apps name for a function.
fn get_azure_container_app_name(prefix: &str, name: &str) -> String {
    format!("{}-{}", prefix, name)
}

/// Get the Key Vault name for importing certificates.
/// For now, we use a simple naming convention. In the future, this could be extracted from infrastructure requirements.
fn get_keyvault_name_for_function(ctx: &ResourceControllerContext<'_>) -> Result<String> {
    // Use the resource prefix to generate a deterministic Key Vault name
    // Azure Key Vault names must be 3-24 characters and only contain alphanumeric characters and hyphens
    let vault_name = format!("{}-kv", ctx.resource_prefix)
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .to_lowercase();

    // Truncate if needed (Key Vault names max 24 chars)
    let vault_name = if vault_name.len() > 24 {
        vault_name[..24].to_string()
    } else {
        vault_name
    };

    Ok(vault_name)
}

/// Domain information for a function.
struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    keyvault_cert_id: Option<String>,
    uses_custom_domain: bool,
}

/// Converts PEM-encoded private key and certificate chain to PKCS#12 format for Azure Key Vault.
/// Azure Key Vault requires certificates in PKCS#12 (PFX) format.
fn pem_to_pkcs12(private_key_pem: &str, certificate_chain_pem: &str) -> Result<Vec<u8>> {
    use alien_error::IntoAlienError;

    // Parse private key PEM
    let key_blocks = pem::parse_many(private_key_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse private key PEM".to_string(),
            resource_id: None,
        })?;

    let key_block = key_blocks
        .into_iter()
        .find(|p| p.tag().ends_with("PRIVATE KEY"))
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message:
                    "No PRIVATE KEY block found in private key PEM (expected BEGIN PRIVATE KEY)"
                        .to_string(),
                resource_id: None,
            })
        })?;

    // p12 expects PKCS#8 PrivateKeyInfo DER bytes (BEGIN PRIVATE KEY)
    if key_block.tag() != "PRIVATE KEY" {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "Unsupported key type '{}'. Expected 'PRIVATE KEY' (PKCS#8). Convert to PKCS#8 first.",
                key_block.tag()
            ),
            resource_id: None,
        }));
    }
    let key_der = key_block.contents().to_vec();

    // Parse certificate chain PEM
    let cert_blocks = pem::parse_many(certificate_chain_pem)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Failed to parse certificate chain PEM".to_string(),
            resource_id: None,
        })?;

    let mut certs: Vec<pem::Pem> = cert_blocks
        .into_iter()
        .filter(|p| p.tag().contains("CERTIFICATE"))
        .collect();

    if certs.is_empty() {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: "No CERTIFICATE blocks found in PEM".to_string(),
            resource_id: None,
        }));
    }

    // Leaf is first, rest are intermediates
    let leaf_pem = certs.remove(0);
    let leaf_der = leaf_pem.contents().to_vec();

    let intermediate_ders: Vec<Vec<u8>> =
        certs.into_iter().map(|p| p.contents().to_vec()).collect();
    let intermediate_refs: Vec<&[u8]> = intermediate_ders.iter().map(|v| v.as_slice()).collect();

    // Build PKCS#12 with empty password
    let pfx = p12::PFX::new_with_cas(
        &leaf_der,
        &key_der,
        &intermediate_refs,
        "",
        "Alien Function Certificate",
    )
    .ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "Failed to build PKCS#12 (p12::PFX::new_with_cas returned None)".to_string(),
            resource_id: None,
        })
    })?;

    Ok(pfx.to_der())
}

// ≡ Controller definition =======================================================
#[controller]
pub struct AzureFunctionController {
    // ─────────── Persisted fields ───────────
    /// Azure Container App name. Filled on *create* and reused for update/delete.
    pub(crate) container_app_name: Option<String>,

    /// Resource ID of the Container App (ARM ID).
    pub(crate) resource_id: Option<String>,

    /// Public URL (if `Ingress::Public`).
    pub(crate) url: Option<String>,

    /// URL returned by Azure ARM for *current* long‑running operation.
    pub(crate) pending_operation_url: Option<String>,
    /// Retry‑after seconds for the current LRO (populated when Azure returns it).
    pub(crate) pending_operation_retry_after: Option<u64>,
    /// Dapr component names for queue triggers (one per queue trigger)
    pub(crate) dapr_components: Vec<String>,

    // Domain & Certificate
    /// The fully qualified domain name for the function
    pub(crate) fqdn: Option<String>,
    /// The certificate ID from the TLS controller
    pub(crate) certificate_id: Option<String>,
    /// The Azure Key Vault certificate ID
    pub(crate) keyvault_cert_id: Option<String>,
    /// Whether this function uses a custom domain
    pub(crate) uses_custom_domain: bool,
    /// Timestamp when certificate was issued (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    // Commands infrastructure
    /// Service Bus namespace name for commands delivery
    pub(crate) commands_namespace_name: Option<String>,
    /// Service Bus queue name for commands delivery
    pub(crate) commands_queue_name: Option<String>,
    /// Dapr component name for commands queue
    pub(crate) commands_dapr_component: Option<String>,
}

// ≡ Lifecycle implementation ===================================================
#[controller]
impl AzureFunctionController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        info!(name=%func_cfg.id, "Initiating Azure Container App function creation");

        // Product limitation: Only allow at most one queue trigger per function
        let queue_trigger_count = func_cfg
            .triggers
            .iter()
            .filter(|trigger| matches!(trigger, alien_core::FunctionTrigger::Queue { .. }))
            .count();

        if queue_trigger_count > 1 {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Function '{}' has {} queue triggers, but only one queue trigger per function is currently supported",
                    func_cfg.id,
                    queue_trigger_count
                ),
                resource_id: Some(func_cfg.id.clone()),
            }));
        }

        // Derive deterministic resource names.
        let container_app_name = get_azure_container_app_name(ctx.resource_prefix, &func_cfg.id);
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;

        // Build ARM request body.
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;
        let container_app = self
            .build_container_app(func_cfg, &environment_name, azure_cfg, ctx)
            .await?;

        // Fire the CREATE/UPDATE call.
        let op_result = client
            .create_or_update_container_app(
                &resource_group_name,
                &container_app_name,
                &container_app,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        // Persist common fields.
        self.container_app_name = Some(container_app_name.clone());

        match op_result {
            OperationResult::Completed(immediate_app) => {
                info!(name=%container_app_name, "Container app creation completed immediately");
                self.handle_creation_completed(ctx, &immediate_app);

                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
                    suggested_delay: None,
                })
            }
            OperationResult::LongRunning(lro) => {
                info!(name=%container_app_name, operation_url=%lro.url, "Container app creation is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForCreateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
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
        let operation_url = match &self.pending_operation_url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: "No pending operation URL recorded in WaitingForCreateOperation"
                        .to_string(),
                    operation: Some("waiting_for_create_operation".to_string()),
                    resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
                }))
            }
        };

        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();

        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let lro = LongRunningOperation {
            url: operation_url.clone(),
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
        };

        // Poll ARM operation.
        let op_status = operation_client
            .check_status(&lro, "CreateContainerApp", container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for container app creation".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if op_status.is_some() {
            info!(name=%container_app_name, "LRO completed – checking resource status");
            Ok(HandlerAction::Continue {
                state: CreatingContainerApp,
                suggested_delay: None,
            })
        } else {
            // Still running – schedule another poll.
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: 100,
                suggested_delay: Some(delay),
            })
        }
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
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let resource_group_name = get_resource_group_name(ctx.state)?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, container_app_name)
            .await
        {
            Ok(app) => {
                if let Some(props) = &app.properties {
                    match props.provisioning_state.as_ref() {
                        Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                            info!(name=%container_app_name, "Provisioning succeeded – configuring Dapr components");
                            self.handle_creation_completed(ctx, &app);

                            // Branch based on ingress type
                            // If public, resolve domain and proceed to certificate flow
                            // If private, skip directly to Dapr component configuration
                            if func_cfg.ingress == Ingress::Public {
                                match Self::resolve_domain_info(ctx, &func_cfg.id) {
                                    Ok(domain_info) => {
                                        info!(fqdn=%domain_info.fqdn, "Resolved domain for public function");
                                        self.fqdn = Some(domain_info.fqdn);
                                        self.certificate_id = domain_info.certificate_id;
                                        self.keyvault_cert_id = domain_info.keyvault_cert_id;
                                        self.uses_custom_domain = domain_info.uses_custom_domain;

                                        // Proceed to certificate flow
                                        return Ok(HandlerAction::Continue {
                                            state: WaitingForCertificate,
                                            suggested_delay: None,
                                        });
                                    }
                                    Err(e) => {
                                        warn!("Failed to resolve domain info, skipping custom domain setup: {}", e);
                                        // Continue without custom domain
                                    }
                                }
                            }

                            Ok(HandlerAction::Continue {
                                state: ConfiguringDaprComponents,
                                suggested_delay: None,
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                            debug!(name=%container_app_name, "Provisioning still in progress");
                            Ok(HandlerAction::Stay {
                                max_times: 60,
                                suggested_delay: Some(Duration::from_secs(10)),
                            })
                        }
                        Some(ContainerAppPropertiesProvisioningState::Failed) => {
                            error!(name=%container_app_name, "Container app provisioning failed");
                            Err(AlienError::new(ErrorData::CloudPlatformError {
                                message: "Container app provisioning failed".to_string(),
                                resource_id: Some(func_cfg.id.clone()),
                            }))
                        }
                        _ => Ok(HandlerAction::Stay {
                            max_times: 60,
                            suggested_delay: Some(Duration::from_secs(10)),
                        }),
                    }
                } else {
                    debug!(name=%container_app_name, "Properties missing – retry");
                    Ok(HandlerAction::Stay {
                        max_times: 60,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                debug!(name=%container_app_name, "Resource not yet visible – retry");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })),
        }
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id));

        let status = metadata.map(|m| &m.certificate_status);

        match status {
            Some(CertificateStatus::Issued) => Ok(HandlerAction::Continue {
                state: ImportingCertificate,
                suggested_delay: None,
            }),
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(function_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(function_config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(function_config.id.clone()),
            })
        })?;

        // Convert PEM to PKCS#12 for Azure Key Vault
        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);

        // Get Key Vault URL from infrastructure requirements
        let keyvault_name = get_keyvault_name_for_function(ctx)?;
        let keyvault_url = format!("https://{}.vault.azure.net", keyvault_name);

        // Import certificate to Key Vault
        let keyvault_client = ctx
            .service_provider
            .get_azure_key_vault_certificates_client(azure_cfg)?;
        let cert_name = format!("{}-{}", ctx.resource_prefix, function_config.id)
            .replace('_', "-") // Key Vault names can't have underscores
            .to_lowercase(); // Convert to lowercase for Key Vault naming requirements

        let import_request = CertificateImportParameters {
            value: pkcs12_base64,
            pwd: Some(String::new()), // Empty password
            policy: None,
            attributes: None,
            tags: HashMap::new(),
            preserve_cert_order: None,
        };

        let response = keyvault_client
            .import_certificate(keyvault_url, cert_name, import_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to Key Vault".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.keyvault_cert_id = response.id;

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            function=%function_config.id,
            cert_id=?self.keyvault_cert_id,
            "Certificate imported to Key Vault"
        );

        Ok(HandlerAction::Continue {
            state: ConfiguringCustomDomain,
            suggested_delay: None,
        })
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Container app name not set".to_string(),
            })
        })?;

        let fqdn = self.fqdn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "FQDN not set".to_string(),
            })
        })?;

        let keyvault_cert_id = self.keyvault_cert_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: function_config.id.clone(),
                message: "Key Vault certificate ID not set".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        // Get the current container app
        let mut app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get container app for custom domain configuration".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        // Update the ingress configuration with custom domain
        if let Some(props) = &mut app.properties {
            if let Some(config) = &mut props.configuration {
                if let Some(ingress) = &mut config.ingress {
                    // Add custom domain configuration
                    ingress.custom_domains = vec![CustomDomain {
                        name: fqdn.clone(),
                        binding_type: Some(CustomDomainBindingType::SniEnabled),
                        certificate_id: Some(keyvault_cert_id.clone()),
                    }];
                }
            }
        }

        // Update the container app with custom domain
        let _operation = client
            .create_or_update_container_app(&resource_group_name, container_app_name, &app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to configure custom domain for container app".to_string(),
                resource_id: Some(function_config.id.clone()),
            })?;

        info!(
            function=%function_config.id,
            fqdn=%fqdn,
            "Custom domain configured for container app"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForDns,
            suggested_delay: Some(Duration::from_secs(2)),
        })
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
        let function_config = ctx.desired_resource_config::<Function>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&function_config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(
                    function=%function_config.id,
                    fqdn=%self.fqdn.as_ref().unwrap_or(&"unknown".to_string()),
                    "DNS record created successfully"
                );
                Ok(HandlerAction::Continue {
                    state: ConfiguringDaprComponents,
                    suggested_delay: None,
                })
            }
            Some(DnsRecordStatus::Failed) => {
                let fqdn = metadata.map(|m| m.fqdn.as_str()).unwrap_or("unknown");
                let detail = metadata
                    .and_then(|m| m.dns_error.as_deref())
                    .unwrap_or("unknown error");
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("DNS record creation failed for {fqdn}: {detail}"),
                    resource_id: Some(function_config.id.clone()),
                }))
            }
            _ => Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(5)),
            }),
        }
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
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        info!(name=%container_app_name, "Configuring Dapr components for queue triggers");

        // Create Dapr components for queue triggers
        let mut created_any = false;
        for trigger in &func_cfg.triggers {
            if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                info!(function=%func_cfg.id, queue=%queue.id, "Creating Dapr Service Bus component");
                self.create_dapr_service_bus_component(ctx, &container_app_name, &func_cfg, queue)
                    .await?;
                created_any = true;
            }
        }

        if !created_any {
            info!(function=%func_cfg.id, "No queue triggers found, skipping Dapr component creation");
        }

        // Go to commands infrastructure next
        Ok(HandlerAction::Continue {
            state: CreatingCommandsInfrastructure,
            suggested_delay: None,
        })
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
        let func_cfg = ctx.desired_resource_config::<Function>()?;

        if !func_cfg.commands_enabled {
            debug!(function=%func_cfg.id, "Commands not enabled, skipping commands infrastructure");
            return Ok(HandlerAction::Continue {
                state: RunningReadinessProbe,
                suggested_delay: None,
            });
        }

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;

        // Get the Service Bus namespace from the dependent resource
        let namespace_ref = ResourceRef::new(
            alien_core::AzureServiceBusNamespace::RESOURCE_TYPE,
            "default-service-bus-namespace",
        );
        let namespace_controller = ctx.require_dependency::<crate::infra_requirements::azure_service_bus_namespace::AzureServiceBusNamespaceController>(&namespace_ref)?;
        let namespace_name = namespace_controller
            .namespace_name
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: func_cfg.id.clone(),
                    dependency_id: namespace_ref.id.clone(),
                })
            })?
            .clone();

        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        // Create commands queue in the Service Bus namespace
        let queue_name = format!("{}-rq", container_app_name);
        let mgmt = ctx
            .service_provider
            .get_azure_service_bus_management_client(azure_config)?;

        info!(
            function=%func_cfg.id,
            namespace=%namespace_name,
            queue=%queue_name,
            "Creating commands Service Bus queue"
        );

        mgmt.create_or_update_queue(
            resource_group_name.clone(),
            namespace_name.clone(),
            queue_name.clone(),
            alien_azure_clients::models::queue::SbQueueProperties {
                accessed_at: None,
                auto_delete_on_idle: None,
                count_details: None,
                created_at: None,
                dead_lettering_on_message_expiration: None,
                default_message_time_to_live: None,
                duplicate_detection_history_time_window: None,
                enable_batched_operations: None,
                enable_express: None,
                enable_partitioning: None,
                forward_dead_lettered_messages_to: None,
                forward_to: None,
                lock_duration: None,
                max_delivery_count: None,
                max_message_size_in_kilobytes: None,
                max_size_in_megabytes: None,
                message_count: None,
                requires_duplicate_detection: None,
                requires_session: None,
                size_in_bytes: None,
                status: None,
                updated_at: None,
            },
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!(
                "Failed to create commands Service Bus queue '{}'",
                queue_name
            ),
            resource_id: Some(func_cfg.id.clone()),
        })?;

        // Create Dapr component for commands queue
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let ns_fqdn = format!("{}.servicebus.windows.net", namespace_name);
        let component_name = format!("servicebus-{}-commands", func_cfg.id);

        let mut metadata = vec![
            DaprMetadata {
                name: Some("namespaceName".into()),
                value: Some(ns_fqdn),
                secret_ref: None,
            },
            DaprMetadata {
                name: Some("consumerID".into()),
                value: Some(format!("{}-commands", func_cfg.id)),
                secret_ref: None,
            },
        ];

        // Add client ID for user-assigned managed identity
        let service_account_id = format!("{}-sa", func_cfg.get_permissions());
        let service_account_ref = alien_core::ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );
        if let Ok(sa_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &sa_state.identity_client_id {
                metadata.push(DaprMetadata {
                    name: Some("azureClientId".into()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("pubsub.azure.servicebus.queues".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata,
                scopes: vec![func_cfg.id.clone()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        info!(
            function=%func_cfg.id,
            component=%component_name,
            "Creating commands Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_config)?;

        client
            .create_or_update_dapr_component(
                &resource_group_name,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create commands Dapr component '{}'",
                    component_name
                ),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        self.commands_namespace_name = Some(namespace_name);
        self.commands_queue_name = Some(queue_name);
        self.commands_dapr_component = Some(component_name);

        info!(function=%func_cfg.id, "Commands Service Bus infrastructure created");

        Ok(HandlerAction::Continue {
            state: RunningReadinessProbe,
            suggested_delay: None,
        })
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
        let func_cfg = ctx.desired_resource_config::<Function>()?;

        // If no readiness probe configured → Skip to applying permissions.
        if func_cfg.readiness_probe.is_none() {
            return Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            });
        }

        // Only run probe for public ingress where we have a URL.
        if func_cfg.ingress != Ingress::Public {
            return Ok(HandlerAction::Continue {
                state: ApplyingPermissions,
                suggested_delay: None,
            });
        }

        let url = match &self.url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL is missing".to_string(),
                }))
            }
        };

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => {
                info!(name=%func_cfg.id, "Readiness probe succeeded");

                Ok(HandlerAction::Continue {
                    state: ApplyingPermissions,
                    suggested_delay: None,
                })
            }
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: READINESS_PROBE_MAX_ATTEMPTS as u32,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
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
        let func_cfg = ctx.desired_resource_config::<Function>()?;

        info!(name=%func_cfg.id, "Applying resource-scoped permissions");

        // Apply resource-scoped permissions from the stack
        if let Some(container_app_name) = &self.container_app_name {
            use crate::core::ResourcePermissionsHelper;
            use alien_azure_clients::authorization::Scope;

            let config = ctx.desired_resource_config::<Function>()?;

            // Build Azure resource scope for the container app
            let resource_scope = Scope::Resource {
                resource_group_name: azure_utils::get_resource_group_name(ctx.state)?,
                resource_provider: "Microsoft.App".to_string(),
                parent_resource_path: None,
                resource_type: "containerApps".to_string(),
                resource_name: container_app_name.to_string(),
            };

            ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
                ctx,
                &config.id,
                container_app_name,
                resource_scope,
                "Function",
                "function",
            )
            .await?;
        }

        info!(name=%func_cfg.id, "Successfully applied resource-scoped permissions");

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: func_cfg.id.clone(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        // Heartbeat check: verify Container App still exists and is in correct state
        let container_app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get Container App during heartbeat check".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        // Verify Container App is in Succeeded state - drift is non-retryable
        if let Some(properties) = &container_app.properties {
            use alien_azure_clients::models::container_apps::ContainerAppPropertiesProvisioningState;
            if properties.provisioning_state
                == Some(ContainerAppPropertiesProvisioningState::Failed)
            {
                return Err(AlienError::new(ErrorData::ResourceDrift {
                    resource_id: func_cfg.id.clone(),
                    message: "Container App is in Failed state".to_string(),
                }));
            }
        }

        // Check for certificate renewal (if using custom domain)
        if self.uses_custom_domain && self.certificate_id.is_some() {
            let metadata = ctx
                .deployment_config
                .domain_metadata
                .as_ref()
                .and_then(|meta| meta.resources.get(&func_cfg.id));

            if let Some(resource) = metadata {
                // Check if certificate has been renewed (issued_at timestamp changed)
                if let Some(new_issued_at) = &resource.issued_at {
                    if self.certificate_issued_at.as_ref() != Some(new_issued_at) {
                        info!(
                            function=%func_cfg.id,
                            old_issued_at=?self.certificate_issued_at,
                            new_issued_at=%new_issued_at,
                            "Certificate renewed, triggering update to re-import certificate"
                        );
                        return Ok(HandlerAction::Continue {
                            state: UpdateStart,
                            suggested_delay: None,
                        });
                    }
                }
            }
        }

        debug!(name = %func_cfg.id, "Heartbeat check passed");
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = UpdateStart,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::InfrastructureError {
                message: "container_app_name missing prior to update_start".to_string(),
                operation: Some("update_start".to_string()),
                resource_id: Some(func_cfg.id.clone()),
            })
        })?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        // Build desired spec
        let desired_app = self
            .build_container_app(func_cfg, &environment_name, azure_cfg, ctx)
            .await?;

        // Issue UPDATE
        let op_result = client
            .update_container_app(&resource_group_name, container_app_name, &desired_app)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to initiate container app update".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        match op_result {
            OperationResult::Completed(_) => {
                info!(name=%container_app_name, "Update completed immediately – polling app status");
                Ok(HandlerAction::Continue {
                    state: UpdatingContainerApp,
                    suggested_delay: Some(Duration::from_secs(3)),
                })
            }
            OperationResult::LongRunning(lro) => {
                info!(name=%container_app_name, operation_url=%lro.url, "Update is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());

                Ok(HandlerAction::Continue {
                    state: WaitingForUpdateOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
        }
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
        let operation_url = match &self.pending_operation_url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: "No pending operation URL recorded in WaitingForUpdateOperation"
                        .to_string(),
                    operation: Some("waiting_for_update_operation".to_string()),
                    resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
                }))
            }
        };

        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
        };

        let op_status = operation_client
            .check_status(&lro, "UpdateContainerApp", container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for container app update".to_string(),
                resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
            })?;

        if op_status.is_some() {
            Ok(HandlerAction::Continue {
                state: UpdatingContainerApp,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: 100,
                suggested_delay: Some(delay),
            })
        }
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
        let azure_cfg = ctx.get_azure_config()?;
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        let app = client
            .get_container_app(&resource_group_name, container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Error checking container app update status".to_string(),
                resource_id: Some(func_cfg.id.clone()),
            })?;

        if let Some(props) = &app.properties {
            match props.provisioning_state.as_ref() {
                Some(ContainerAppPropertiesProvisioningState::Succeeded) => {
                    info!(name=%container_app_name, "Update provisioning succeeded – updating Dapr components");

                    let container_app_url = self.extract_url_from_container_app(&app);

                    // Check for URL override in deployment config, otherwise use Container App URL
                    self.url = ctx
                        .deployment_config
                        .public_urls
                        .as_ref()
                        .and_then(|urls| urls.get(&func_cfg.id).cloned())
                        .or(container_app_url);

                    Ok(HandlerAction::Continue {
                        state: UpdateDaprComponents,
                        suggested_delay: None,
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::InProgress) => {
                    Ok(HandlerAction::Stay {
                        max_times: 60,
                        suggested_delay: Some(Duration::from_secs(10)),
                    })
                }
                Some(ContainerAppPropertiesProvisioningState::Failed) => {
                    Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: "Container app update failed".to_string(),
                        resource_id: Some(func_cfg.id.clone()),
                    }))
                }
                _ => Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(10)),
                }),
            }
        } else {
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
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
        let current_config = ctx.desired_resource_config::<Function>()?;
        let previous_config = ctx.previous_resource_config::<Function>()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        // Check if triggers have changed
        let triggers_changed = current_config.triggers != previous_config.triggers;

        if triggers_changed {
            info!(function=%current_config.id, "Function triggers changed, updating Dapr components");

            // For simplicity, we'll delete old components and create new ones
            // In a production system, you might want to do a more sophisticated diff
            self.delete_all_dapr_components(ctx).await?;

            // Create new components for ALL queue triggers
            for trigger in &current_config.triggers {
                if let alien_core::FunctionTrigger::Queue { queue } = trigger {
                    self.create_dapr_service_bus_component(
                        ctx,
                        &container_app_name,
                        &current_config,
                        queue,
                    )
                    .await?;
                }
            }
        } else {
            info!(function=%current_config.id, "No trigger changes detected");
        }

        // Always go to readiness probe next (linear flow)
        Ok(HandlerAction::Continue {
            state: UpdateRunningReadinessProbe,
            suggested_delay: None,
        })
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
        // Re‑use the same readiness‑probe helper.
        let func_cfg = ctx.desired_resource_config::<Function>()?;
        if func_cfg.readiness_probe.is_none() || func_cfg.ingress != Ingress::Public {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let url = self
            .url
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceControllerConfigError {
                    resource_id: func_cfg.id.clone(),
                    message: "Readiness probe configured but URL missing after update".to_string(),
                })
            })?
            .clone();

        match run_readiness_probe(ctx, &url).await {
            Ok(()) => Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            }),
            Err(_) => {
                // Probe failed, let the framework handle retries
                Ok(HandlerAction::Stay {
                    max_times: READINESS_PROBE_MAX_ATTEMPTS as u32,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let func_cfg = ctx.desired_resource_config::<Function>()?;

        // Handle case where container_app_name is not set (e.g., creation failed early)
        let _container_app_name = match self.container_app_name.as_ref() {
            Some(name) => name.clone(),
            None => {
                // No container app was created, nothing to delete
                info!(resource_id=%func_cfg.id, "No Container App to delete - creation failed early");

                // Clear any remaining state and mark as deleted
                self.clear_all();

                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };

        // Always go to deleting Dapr components first (linear flow)
        Ok(HandlerAction::Continue {
            state: DeletingDaprComponents,
            suggested_delay: None,
        })
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
        let function_config = ctx.desired_resource_config::<Function>()?;

        info!(function=%function_config.id, components=?self.dapr_components, "Deleting Dapr components");

        // Delete all Dapr components using best-effort approach (ignore NotFound)
        self.delete_all_dapr_components(ctx).await?;

        // Continue to commands infrastructure cleanup
        Ok(HandlerAction::Continue {
            state: DeletingCommandsInfrastructure,
            suggested_delay: None,
        })
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
        let azure_config = ctx.get_azure_config()?;

        // Delete commands Dapr component (best-effort)
        if let Some(component_name) = self.commands_dapr_component.take() {
            let _ = azure_config;
            warn!(
                component=%component_name,
                "Skipping commands Dapr component deletion because the Azure Container Apps client does not expose a delete API"
            );
        }

        // Delete commands Service Bus queue (best-effort)
        if let (Some(namespace_name), Some(queue_name)) = (
            self.commands_namespace_name.take(),
            self.commands_queue_name.take(),
        ) {
            let resource_group_name = get_resource_group_name(ctx.state)?;
            info!(namespace=%namespace_name, queue=%queue_name, "Deleting commands Service Bus queue");
            let mgmt = ctx
                .service_provider
                .get_azure_service_bus_management_client(azure_config)?;
            match mgmt
                .delete_queue(
                    resource_group_name,
                    namespace_name.clone(),
                    queue_name.clone(),
                )
                .await
            {
                Ok(_) => {
                    info!(queue=%queue_name, "Commands Service Bus queue deleted");
                }
                Err(e) => {
                    warn!(
                        queue=%queue_name,
                        error=%e,
                        "Failed to delete commands Service Bus queue (may already be deleted)"
                    );
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingApp,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingApp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_app(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceControllerConfigError {
                resource_id: ctx.desired_config.id().to_string(),
                message: "Container app name not set in state".to_string(),
            })
        })?;

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .delete_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Ok(OperationResult::Completed(_)) => {
                info!(name=%container_app_name, "Container app deleted immediately");
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(OperationResult::LongRunning(lro)) => {
                info!(name=%container_app_name, operation_url=%lro.url, "Deletion is long‑running");
                self.pending_operation_url = Some(lro.url.clone());
                self.pending_operation_retry_after = lro.retry_after.map(|d| d.as_secs());
                Ok(HandlerAction::Continue {
                    state: WaitingForDeleteOperation,
                    suggested_delay: Some(lro.retry_after.unwrap_or(Duration::from_secs(15))),
                })
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app already deleted");
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Err(e) => {
                let function_config = ctx.desired_resource_config::<Function>()?;
                Err(e.context(ErrorData::CloudPlatformError {
                    message: "Failed to delete container app".to_string(),
                    resource_id: Some(function_config.id.clone()),
                }))
            }
        }
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
        let operation_url = match &self.pending_operation_url {
            Some(u) => u.clone(),
            None => {
                return Err(AlienError::new(ErrorData::InfrastructureError {
                    message: "No pending_operation_url in WaitingForDeleteOperation".to_string(),
                    operation: Some("waiting_for_delete_operation".to_string()),
                    resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
                }))
            }
        };

        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = self.container_app_name.as_ref().unwrap();
        let operation_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let lro = LongRunningOperation {
            url: operation_url,
            retry_after: self.pending_operation_retry_after.map(Duration::from_secs),
        };

        let op_status = operation_client
            .check_status(&lro, "DeleteContainerApp", container_app_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Azure ARM operation failed for container app deletion".to_string(),
                resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
            })?;

        if op_status.is_some() {
            Ok(HandlerAction::Continue {
                state: DeletingContainerApp,
                suggested_delay: None,
            })
        } else {
            let delay = self
                .pending_operation_retry_after
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(15));
            Ok(HandlerAction::Stay {
                max_times: 60,
                suggested_delay: Some(delay),
            })
        }
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
        let azure_cfg = ctx.get_azure_config()?;
        let container_app_name = match &self.container_app_name {
            Some(n) => n.clone(),
            None => {
                // Already cleared → consider successful
                self.clear_all();
                return Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                });
            }
        };
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let client = ctx
            .service_provider
            .get_azure_container_apps_client(azure_cfg)?;

        match client
            .get_container_app(&resource_group_name, &container_app_name)
            .await
        {
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                info!(name=%container_app_name, "Container app confirmed deleted");
                self.clear_all();
                Ok(HandlerAction::Continue {
                    state: Deleted,
                    suggested_delay: None,
                })
            }
            Ok(_) => {
                debug!(name=%container_app_name, "Container app still exists – retry");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(15)),
                })
            }
            Err(e) => Err(e.context(ErrorData::CloudPlatformError {
                message: "Error checking container app deletion status".to_string(),
                resource_id: Some(ctx.desired_resource_config::<Function>()?.id.clone()),
            })),
        }
    }

    // ─────────────── TERMINALS ────────────────────────────────

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
            // If we have a custom domain, use the FQDN
            // Otherwise, use the Container App URL
            let load_balancer_endpoint = if let Some(fqdn) = &self.fqdn {
                Some(alien_core::LoadBalancerEndpoint {
                    dns_name: fqdn.clone(),
                    hosted_zone_id: None, // Azure doesn't use hosted zones like AWS
                })
            } else {
                self.url
                    .as_ref()
                    .map(|url| alien_core::LoadBalancerEndpoint {
                        dns_name: url.clone(),
                        hosted_zone_id: None,
                    })
            };

            ResourceOutputs::new(FunctionOutputs {
                function_name: self
                    .container_app_name
                    .clone()
                    .unwrap_or_else(|| "function-name-placeholder".to_string()),
                url: self.url.clone(),
                identifier: Some(id.clone()),
                load_balancer_endpoint,
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
        use alien_core::bindings::{BindingValue, ContainerAppFunctionBinding, FunctionBinding};

        if let (Some(container_app_name), Some(resource_id)) =
            (&self.container_app_name, &self.resource_id)
        {
            // Extract resource group name from ARM resource ID
            // Format: /subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.App/containerApps/{name}
            let resource_group_name = resource_id
                .split('/')
                .nth(4)
                .unwrap_or("unknown-resource-group")
                .to_string();

            let binding = FunctionBinding::ContainerApp(ContainerAppFunctionBinding {
                subscription_id: BindingValue::Value("unknown-subscription".to_string()), // TODO: Store in controller
                resource_group_name: BindingValue::Value(resource_group_name),
                container_app_name: BindingValue::Value(container_app_name.clone()),
                private_url: BindingValue::Value("unknown-private-url".to_string()), // TODO: Store in controller
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

impl AzureFunctionController {
    // ─────────────── HELPER METHODS ────────────────────────────

    /// Resolve domain information for a public function.
    /// Returns either custom domain config or auto-generated domain from metadata.
    fn resolve_domain_info(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<DomainInfo> {
        let stack_settings = &ctx.deployment_config.stack_settings;

        // Check for custom domain configuration
        if let Some(custom) = stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id))
        {
            let keyvault_cert_id = custom
                .certificate
                .azure
                .as_ref()
                .map(|cert| cert.key_vault_certificate_id.clone())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Custom domain requires an Azure Key Vault certificate ID"
                            .to_string(),
                        resource_id: Some(resource_id.to_string()),
                    })
                })?;

            return Ok(DomainInfo {
                fqdn: custom.domain.clone(),
                certificate_id: None,
                keyvault_cert_id: Some(keyvault_cert_id),
                uses_custom_domain: true,
            });
        }

        // Use auto-generated domain from domain metadata
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for public resource".to_string(),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        let resource = metadata.resources.get(resource_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Domain metadata missing for resource".to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        Ok(DomainInfo {
            fqdn: resource.fqdn.clone(),
            certificate_id: Some(resource.certificate_id.clone()),
            keyvault_cert_id: None,
            uses_custom_domain: false,
        })
    }

    fn clear_all(&mut self) {
        self.container_app_name = None;
        self.resource_id = None;
        self.url = None;
        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
        self.dapr_components.clear();
    }

    /// Called whenever provisioning *succeeds* and we have the live resource.
    fn handle_creation_completed(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        app: &ContainerApp,
    ) {
        self.resource_id = app.id.clone();

        let container_app_url = self.extract_url_from_container_app(app);

        // Check for URL override in deployment config, otherwise use Container App URL
        if let Ok(config) = ctx.desired_resource_config::<Function>() {
            self.url = ctx
                .deployment_config
                .public_urls
                .as_ref()
                .and_then(|urls| urls.get(&config.id).cloned())
                .or(container_app_url);
        } else {
            self.url = container_app_url;
        }

        self.pending_operation_url = None;
        self.pending_operation_retry_after = None;
    }

    fn extract_url_from_container_app(&self, app: &ContainerApp) -> Option<String> {
        let fqdn = app
            .properties
            .as_ref()?
            .configuration
            .as_ref()?
            .ingress
            .as_ref()?
            .fqdn
            .clone()?;

        if fqdn.starts_with("http://") || fqdn.starts_with("https://") {
            Some(fqdn)
        } else {
            Some(format!("https://{}", fqdn))
        }
    }

    /// Prepare environment variables using the shared logic, then convert to Azure's EnvironmentVar format
    async fn prepare_environment_variables_azure(
        &self,
        func: &Function,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<EnvironmentVar>> {
        // Get the function's own binding params (may be None during initial creation)
        let self_binding_params = self.get_binding_params()?;

        // Build complete environment using shared logic
        // IMPORTANT: Start with func.environment which includes injected vars from DeploymentConfig
        let complete_env = EnvironmentVariableBuilder::new(&func.environment)
            .add_standard_alien_env_vars(ctx)
            .add_function_transport_env_vars(ctx.platform)
            .add_env_var("ALIEN_RUNTIME_SEND_OTLP".to_string(), "true".to_string())
            .add_linked_resources(&func.links, ctx, &func.id)
            .await?
            .add_self_function_binding(&func.id, self_binding_params.as_ref())?
            .build();

        // Add managed identity environment variable from ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        let mut env_vars = Vec::new();

        // Convert all environment variables to Azure format
        for (name, value) in complete_env {
            env_vars.push(EnvironmentVar {
                name: Some(name),
                value: Some(value),
                secret_ref: None,
            });
        }

        // Add Azure-specific managed identity client ID
        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(client_id) = &service_account_state.identity_client_id {
                env_vars.push(EnvironmentVar {
                    name: Some("AZURE_CLIENT_ID".to_string()),
                    value: Some(client_id.clone()),
                    secret_ref: None,
                });
            }
        }

        Ok(env_vars)
    }

    /// Build the full ContainerApps ARM spec for *desired* state.
    async fn build_container_app(
        &self,
        func: &Function,
        environment_name: &str,
        azure_cfg: &AzureClientConfig,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<ContainerApp> {
        let location = azure_cfg.region.as_deref().unwrap_or("East US");

        let image = match &func.code {
            alien_core::FunctionCode::Image { image } => image.clone(),
            alien_core::FunctionCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Function '{}' uses source code, but only pre‑built images are supported on Azure",
                        func.id
                    ),
                    resource_id: Some(func.id.clone()),
                }));
            }
        };

        // Prepare environment variables using shared logic
        let env_vars = self.prepare_environment_variables_azure(func, ctx).await?;

        // Azure Container Apps requires specific CPU/memory combinations.
        // The ratio is 0.5 Gi per 0.25 CPU (2 Gi per 1 CPU).
        let memory_gi = func.memory_mb as f64 / 1024.0;
        // Azure Container Apps requires specific CPU/memory pairs where CPU = memory_gi / 2.
        // The FunctionMemoryCheck preflight validates that memory_mb is a valid Azure value
        // (512, 1024, 1536, 2048, 2560, 3072, 3584, 4096).
        let cpu = memory_gi / 2.0;

        let container = Container {
            name: Some("main".to_string()),
            image: Some(image.clone()),
            resources: Some(ContainerResources {
                cpu: Some(cpu),
                memory: Some(format!("{}Gi", memory_gi)),
                ephemeral_storage: None,
            }),
            env: env_vars,
            args: vec![],
            command: vec![],
            probes: vec![],
            volume_mounts: vec![],
        };

        // Tags for traceability
        let mut tags = HashMap::new();
        tags.insert("alien-resource".to_string(), "function".to_string());
        tags.insert("alien-function-id".to_string(), func.id.clone());
        tags.insert("alien-stack".to_string(), ctx.resource_prefix.to_string());

        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.App/managedEnvironments/{}",
            azure_cfg.subscription_id, resource_group_name, environment_name
        );

        let ingress_cfg = if func.ingress == Ingress::Public {
            Some(alien_azure_clients::models::container_apps::Ingress {
                external: true,
                target_port: Some(8080),
                traffic: vec![TrafficWeight {
                    weight: Some(100),
                    latest_revision: true,
                    revision_name: None,
                    label: None,
                }],
                transport: IngressTransport::Auto,
                allow_insecure: false,
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
                fqdn: None,
            })
        } else {
            None
        };

        // Registry creds if provided via deployment config
        let mut registries = vec![];
        let mut secrets = vec![];
        let has_password_creds;

        // Get image pull credentials from context (passed from DeploymentConfig)
        if let Some(creds) = &ctx.deployment_config.image_pull_credentials {
            has_password_creds = true;
            let pwd_secret_name = format!("{}-registry-password", func.id);
            secrets.push(Secret {
                identity: None,
                key_vault_url: None,
                name: Some(pwd_secret_name.clone()),
                value: Some(creds.password.clone()),
            });
            let server = if image.contains('/') && !image.starts_with("docker.io/") {
                image.split('/').next().unwrap_or("docker.io").to_string()
            } else {
                "docker.io".to_string()
            };
            registries.push(RegistryCredentials {
                identity: None,
                password_secret_ref: Some(pwd_secret_name),
                server: Some(server),
                username: Some(creds.username.clone()),
            });
        } else {
            has_password_creds = false;
        }

        // Managed identity support from ServiceAccounts
        // Collect all ServiceAccounts:
        // 1. Permission-based ServiceAccount (from permission profile)
        // 2. Linked ServiceAccounts (from function.links)
        use alien_azure_clients::models::container_apps::{
            ManagedServiceIdentity, ManagedServiceIdentityType, UserAssignedIdentities,
            UserAssignedIdentity,
        };

        let mut identity_map = HashMap::new();

        // Add permission-based ServiceAccount
        let service_account_id = format!("{}-sa", func.get_permissions());
        let service_account_ref = ResourceRef::new(
            alien_core::ServiceAccount::RESOURCE_TYPE,
            service_account_id.to_string(),
        );

        if let Ok(service_account_state) = ctx
            .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            )
        {
            if let Some(identity_id) = &service_account_state.identity_resource_id {
                identity_map.insert(
                    identity_id.clone(),
                    UserAssignedIdentity {
                        client_id: None,
                        principal_id: None,
                    },
                );
            }
        }

        // Add linked ServiceAccounts
        for link in &func.links {
            if link.resource_type() == &alien_core::ServiceAccount::RESOURCE_TYPE {
                if let Ok(linked_sa_state) = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                    link,
                ) {
                    if let Some(identity_id) = &linked_sa_state.identity_resource_id {
                        identity_map.insert(
                            identity_id.clone(),
                            UserAssignedIdentity {
                                client_id: None,
                                principal_id: None,
                            },
                        );
                    }
                }
            }
        }

        // If image is from Azure Container Registry and no password credentials were provided,
        // configure the managed identity for ACR pull via the registries field.
        if !has_password_creds && image.contains(".azurecr.io") {
            let acr_server = image.split('/').next().unwrap_or_default().to_string();
            // Use the permission-based service account identity for ACR pull
            if let Ok(service_account_state) = ctx
                .require_dependency::<crate::service_account::AzureServiceAccountController>(
                &service_account_ref,
            ) {
                if let Some(identity_id) = &service_account_state.identity_resource_id {
                    registries.push(RegistryCredentials {
                        identity: Some(identity_id.clone()),
                        password_secret_ref: None,
                        server: Some(acr_server),
                        username: None,
                    });
                }
            }
        }

        // Create managed identity spec if we have any identities
        let identity_resource_ids: Vec<String> = identity_map.keys().cloned().collect();

        let managed_identity = if !identity_map.is_empty() {
            Some(ManagedServiceIdentity {
                principal_id: None,
                tenant_id: None,
                type_: ManagedServiceIdentityType::UserAssigned,
                user_assigned_identities: Some(UserAssignedIdentities(identity_map)),
            })
        } else {
            None
        };

        // Configure Dapr if function has queue triggers
        let dapr_config = if func
            .triggers
            .iter()
            .any(|trigger| matches!(trigger, alien_core::FunctionTrigger::Queue { .. }))
        {
            use alien_azure_clients::models::container_apps::{Dapr, DaprAppProtocol};

            Some(Dapr {
                app_id: Some(func.id.clone()),
                app_port: Some(8080), // Port that alien-runtime listens on
                app_protocol: DaprAppProtocol::Http,
                enable_api_logging: Some(false),
                enabled: true,
                http_max_request_size: None,
                http_read_buffer_size: None,
                log_level: None,
            })
        } else {
            None
        };

        let configuration = Configuration {
            active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
            dapr: dapr_config,
            identity_settings: identity_resource_ids
                .iter()
                .map(|identity_id| IdentitySettings {
                    identity: identity_id.clone(),
                    lifecycle: IdentitySettingsLifecycle::All,
                })
                .collect(),
            ingress: ingress_cfg,
            max_inactive_revisions: None,
            registries,
            runtime: None,
            secrets,
            service: None,
        };

        let template = Template {
            containers: vec![container],
            init_containers: vec![],
            revision_suffix: None,
            scale: Some(Scale {
                cooldown_period: None,
                max_replicas: func.concurrency_limit.map(|c| c as i32).unwrap_or(10),
                min_replicas: Some(if func.ingress == Ingress::Private {
                    0
                } else {
                    1
                }),
                polling_interval: None,
                rules: vec![],
            }),
            service_binds: vec![],
            termination_grace_period_seconds: None,
            volumes: vec![],
        };

        Ok(ContainerApp {
            extended_location: None,
            id: None,
            identity: managed_identity,
            location: location.to_string(),
            managed_by: None,
            name: None,
            properties: Some(ContainerAppProperties {
                configuration: Some(configuration),
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: Some(environment_id),
                outbound_ip_addresses: vec![],
                provisioning_state: None,
                running_status: None,
                template: Some(template),
                workload_profile_name: None,
            }),
            system_data: None,
            tags,
            type_: None,
        })
    }

    /// Creates a Dapr Service Bus component for a queue trigger
    async fn create_dapr_service_bus_component(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        _container_app_name: &str,
        function_config: &alien_core::Function,
        queue_ref: &alien_core::ResourceRef,
    ) -> Result<()> {
        use alien_azure_clients::models::managed_environments_dapr_components::{
            DaprComponent, DaprComponentProperties, DaprMetadata,
        };

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;

        // Get queue controller to access Service Bus namespace
        let queue_controller =
            ctx.require_dependency::<crate::queue::azure::AzureQueueController>(queue_ref)?;
        let namespace = queue_controller.namespace_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::DependencyNotReady {
                resource_id: function_config.id.clone(),
                dependency_id: queue_ref.id.clone(),
            })
        })?;
        let ns_fqdn = format!("{}.servicebus.windows.net", namespace);

        // Generate component name: servicebus-function-queue
        let component_name = format!("servicebus-{}-{}", function_config.id, queue_ref.id);

        // Create Dapr Service Bus component
        let dapr_component = DaprComponent {
            name: Some(component_name.clone()),
            properties: Some(DaprComponentProperties {
                component_type: Some("pubsub.azure.servicebus.queues".to_string()),
                ignore_errors: false,
                init_timeout: None,
                version: Some("v1".to_string()),
                metadata: {
                    let mut metadata = vec![
                        // Using Microsoft Entra/Managed Identity
                        DaprMetadata {
                            name: Some("namespaceName".into()),
                            value: Some(ns_fqdn),
                            secret_ref: None,
                        },
                        // Optional but handy to control competing-consumer group
                        DaprMetadata {
                            name: Some("consumerID".into()),
                            value: Some(function_config.id.clone()),
                            secret_ref: None,
                        },
                    ];

                    // Add client ID for user-assigned managed identity
                    let service_account_id = format!("{}-sa", function_config.get_permissions());
                    let service_account_ref = alien_core::ResourceRef::new(
                        alien_core::ServiceAccount::RESOURCE_TYPE,
                        service_account_id.to_string(),
                    );

                    if let Ok(service_account_state) = ctx.require_dependency::<crate::service_account::AzureServiceAccountController>(&service_account_ref) {
                        if let Some(client_id) = &service_account_state.identity_client_id {
                            metadata.push(DaprMetadata { 
                                name: Some("azureClientId".into()), 
                                value: Some(client_id.clone()), 
                                secret_ref: None 
                            });
                        }
                    }

                    metadata
                },
                scopes: vec![function_config.id.clone()],
                secret_store_component: None,
                secrets: vec![],
            }),
            id: None,
            system_data: None,
            type_: None,
        };

        info!(
            function=%function_config.id,
            queue=%queue_ref.id,
            component=%component_name,
            environment=%environment_name,
            "Creating Dapr Service Bus component"
        );

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        client
            .create_or_update_dapr_component(
                &resource_group_name,
                &environment_name,
                &component_name,
                &dapr_component,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create Dapr component '{}' for queue '{}'",
                    component_name, queue_ref.id
                ),
                resource_id: Some(function_config.id.clone()),
            })?;

        self.dapr_components.push(component_name.clone());

        info!(
            function=%function_config.id,
            component=%component_name,
            "Successfully created Dapr Service Bus component"
        );

        Ok(())
    }

    /// Deletes all Dapr components using best-effort approach
    async fn delete_all_dapr_components(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        if self.dapr_components.is_empty() {
            return Ok(());
        }

        let azure_config = ctx.get_azure_config()?;
        let resource_group_name = get_resource_group_name(ctx.state)?;
        let environment_name = get_container_apps_environment_name(ctx.state)?;
        let function_config = ctx.desired_resource_config::<Function>()?;

        let client = ctx
            .service_provider
            .get_azure_container_apps_client(&azure_config)?;

        for component_name in &self.dapr_components.clone() {
            // Check if the component exists (since there's no delete API, we just verify it exists)
            match client
                .get_dapr_component(&resource_group_name, &environment_name, component_name)
                .await
            {
                Ok(_) => {
                    // Component exists - in a full implementation, we would delete it here
                    // For now, we log that manual cleanup may be needed
                    warn!(
                        function=%function_config.id,
                        component=%component_name,
                        environment=%environment_name,
                        "Dapr component exists but no delete API available - may require manual cleanup"
                    );
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(
                        function=%function_config.id,
                        component=%component_name,
                        "Dapr component was already deleted or doesn't exist"
                    );
                }
                Err(e) => {
                    warn!(
                        function=%function_config.id,
                        component=%component_name,
                        error=%e,
                        "Failed to check Dapr component status during deletion"
                    );
                }
            }
        }

        self.dapr_components.clear();
        Ok(())
    }

    /// Creates a controller in a ready state with mock values for testing purposes.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(function_name: &str) -> Self {
        Self {
            state: AzureFunctionState::Ready,
            container_app_name: Some(function_name.to_string()),
            resource_id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                function_name
            )),
            url: Some(format!("https://{}.azurecontainerapps.io", function_name)),
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! # Azure Function Controller Tests
    //!
    //! See `crate::core::controller_test` for a comprehensive guide on testing infrastructure controllers.

    use std::sync::Arc;
    use std::time::Duration;

    use alien_azure_clients::models::container_apps::{
        Configuration, ConfigurationActiveRevisionsMode, ContainerApp, ContainerAppProperties,
        ContainerAppPropertiesProvisioningState, IngressTransport, TrafficWeight,
    };
    use alien_azure_clients::{
        container_apps::MockContainerAppsApi,
        long_running_operation::{
            LongRunningOperation, MockLongRunningOperationApi, OperationResult,
        },
    };
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::{Function, FunctionOutputs, Ingress, Platform, ResourceStatus};
    use alien_error::AlienError;
    use httpmock::MockServer;
    use rstest::rstest;

    use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
    use crate::function::{
        fixtures::*, readiness_probe::test_utils::create_readiness_probe_mock,
        AzureFunctionController,
    };
    use crate::AzureFunctionState;

    fn create_successful_container_app_response(app_name: &str, has_url: bool) -> ContainerApp {
        let fqdn = if has_url {
            Some(format!("{}.azurecontainerapps.io", app_name))
        } else {
            None
        };

        let ingress = if has_url {
            Some(alien_azure_clients::models::container_apps::Ingress {
                external: true,
                target_port: Some(8080),
                fqdn: fqdn.clone(),
                traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
                    latest_revision: true,
                    weight: Some(100),
                    revision_name: None,
                    label: None,
                }],
                transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
                allow_insecure: false,
                additional_port_mappings: vec![],
                custom_domains: vec![],
                ip_security_restrictions: vec![],
                cors_policy: None,
                client_certificate_mode: None,
                exposed_port: None,
                sticky_sessions: None,
            })
        } else {
            None
        };

        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: vec![],
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    fn create_in_progress_container_app_response(app_name: &str) -> ContainerApp {
        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::InProgress),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
                configuration: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    fn setup_mock_client_for_creation_and_update(
        app_name: &str,
        has_url: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock successful app creation - immediate completion
        let app_name = app_name.to_string();
        let app_name_for_create = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_create, has_url),
                ))
            });

        // Mock successful updates - immediate completion
        let app_name_for_update = app_name.clone();
        mock_container_apps
            .expect_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_update, has_url),
                ))
            });

        // Mock get operations - may be called multiple times during creation and update flows
        let app_name_for_get = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get,
                    has_url,
                ))
            })
            .times(0..); // Allow 0 or more calls

        Arc::new(mock_container_apps)
    }

    fn setup_mock_client_for_creation_and_deletion(
        app_name: &str,
        has_url: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock successful app creation - immediate completion
        let app_name = app_name.to_string();
        let app_name_for_create = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_create, has_url),
                ))
            });

        // Mock successful deletion - immediate completion
        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Mock get operations during creation (may be called multiple times)
        let app_name_for_get_creation = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get_creation,
                    has_url,
                ))
            })
            .times(0..); // Allow 0 or more calls during creation

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        Arc::new(mock_container_apps)
    }

    fn setup_mock_client_for_long_running_creation(
        app_name: &str,
        has_url: bool,
    ) -> (Arc<MockContainerAppsApi>, Arc<MockLongRunningOperationApi>) {
        let mut mock_container_apps = MockContainerAppsApi::new();
        let mut mock_lro = MockLongRunningOperationApi::new();

        // Mock creation that starts as long-running
        // Use minimal retry_after for fast tests (actual Azure would use seconds)
        let app_name = app_name.to_string();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(|_, _, _| {
                Ok(OperationResult::LongRunning(LongRunningOperation {
                    url: "https://management.azure.com/subscriptions/.../operations/test-op"
                        .to_string(),
                    retry_after: Some(Duration::from_millis(10)),
                }))
            });

        // Mock LRO polling - first incomplete, then complete
        mock_lro
            .expect_check_status()
            .returning(|_, _, _| Ok(None)) // Still running
            .times(1);

        mock_lro
            .expect_check_status()
            .returning(|_, _, _| Ok(Some("completed".to_string()))) // Completed
            .times(1);

        // Mock get operations showing progression
        let app_name_for_get1 = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_in_progress_container_app_response(
                    &app_name_for_get1,
                ))
            })
            .times(1);

        let app_name_for_get2 = app_name.clone();
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| {
                Ok(create_successful_container_app_response(
                    &app_name_for_get2,
                    has_url,
                ))
            });

        (Arc::new(mock_container_apps), Arc::new(mock_lro))
    }

    fn setup_mock_client_for_best_effort_deletion(
        _app_name: &str,
        app_missing: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock deletion (might fail if app missing)
        if app_missing {
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "ContainerApp".to_string(),
                            resource_name: "test-app".to_string(),
                        },
                    ))
                });
        } else {
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| Ok(OperationResult::Completed(())));
        }

        // Always return not found for final status check
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        Arc::new(mock_container_apps)
    }

    fn setup_mock_service_provider(
        mock_container_apps: Arc<MockContainerAppsApi>,
        mock_lro: Option<Arc<MockLongRunningOperationApi>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut mock_provider = MockPlatformServiceProvider::new();

        mock_provider
            .expect_get_azure_container_apps_client()
            .returning(move |_| Ok(mock_container_apps.clone()));

        if let Some(lro_client) = mock_lro {
            mock_provider
                .expect_get_azure_long_running_operation_client()
                .returning(move |_| Ok(lro_client.clone()));
        }

        // Mock Azure authorization client for resource-scoped permissions
        mock_provider
            .expect_get_azure_authorization_client()
            .returning(|_| {
                use alien_azure_clients::authorization::MockAuthorizationApi;
                Ok(Arc::new(MockAuthorizationApi::new()))
            });

        Arc::new(mock_provider)
    }

    /// Sets up mock Container Apps client and optional readiness probe mock server
    /// Returns (container_apps_mock_provider, optional_mock_server)
    fn setup_mocks_for_function(
        function: &Function,
        app_name: &str,
        for_deletion: bool,
    ) -> (Arc<MockPlatformServiceProvider>, Option<MockServer>) {
        let has_url = function.ingress == Ingress::Public;
        let needs_readiness_probe = has_url && function.readiness_probe.is_some();

        // Set up mock server for readiness probe if needed
        let mock_server = if needs_readiness_probe {
            Some(create_readiness_probe_mock(function))
        } else {
            None
        };

        // Set up Container Apps client mock - create custom response if we need to override URL
        let container_apps_mock = if needs_readiness_probe && mock_server.is_some() {
            // Create custom mock that returns the mock server URL
            let mock_server_url = mock_server.as_ref().unwrap().base_url();
            setup_mock_client_with_custom_url(app_name, &mock_server_url, for_deletion)
        } else if for_deletion {
            setup_mock_client_for_creation_and_deletion(app_name, has_url)
        } else {
            setup_mock_client_for_creation_and_update(app_name, has_url)
        };

        let mock_provider = setup_mock_service_provider(container_apps_mock, None);

        (mock_provider, mock_server)
    }

    fn setup_mock_client_with_custom_url(
        app_name: &str,
        custom_url: &str,
        for_deletion: bool,
    ) -> Arc<MockContainerAppsApi> {
        let mut mock_container_apps = MockContainerAppsApi::new();

        // Create a container app response with custom URL
        let custom_response = create_container_app_with_custom_url(app_name, custom_url);

        // Mock successful app creation
        let response_for_create = custom_response.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| Ok(OperationResult::Completed(response_for_create.clone())));

        if for_deletion {
            // Mock successful deletion
            mock_container_apps
                .expect_delete_container_app()
                .returning(|_, _| Ok(OperationResult::Completed(())));

            // Mock get operations during creation (may be called multiple times)
            let response_for_get_creation = custom_response.clone();
            mock_container_apps
                .expect_get_container_app()
                .returning(move |_, _| Ok(response_for_get_creation.clone()))
                .times(0..);

            // Mock get operation failure for deletion verification
            mock_container_apps
                .expect_get_container_app()
                .returning(|_, _| {
                    Err(AlienError::new(
                        CloudClientErrorData::RemoteResourceNotFound {
                            resource_type: "ContainerApp".to_string(),
                            resource_name: "test-app".to_string(),
                        },
                    ))
                })
                .times(0..);
        } else {
            // Mock successful updates
            let response_for_update = custom_response.clone();
            mock_container_apps
                .expect_update_container_app()
                .returning(move |_, _, _| {
                    Ok(OperationResult::Completed(response_for_update.clone()))
                });

            // Mock get operations (may be called multiple times)
            let response_for_get = custom_response.clone();
            mock_container_apps
                .expect_get_container_app()
                .returning(move |_, _| Ok(response_for_get.clone()))
                .times(0..);
        }

        Arc::new(mock_container_apps)
    }

    fn create_container_app_with_custom_url(app_name: &str, custom_url: &str) -> ContainerApp {
        // For tests, just extract the host and port from the URL string
        let url_without_protocol = custom_url.strip_prefix("http://").unwrap_or(custom_url);
        let (host, _port) = if let Some(colon_pos) = url_without_protocol.find(':') {
            let host = &url_without_protocol[..colon_pos];
            let port_str = &url_without_protocol[colon_pos + 1..];
            let port = port_str.parse::<u16>().unwrap_or(80);
            (host, Some(port))
        } else {
            (url_without_protocol, None)
        };

        // Create FQDN that matches the custom URL
        let _fqdn = if let Some(port) = _port {
            format!("{}:{}", host, port)
        } else {
            host.to_string()
        };

        let ingress = Some(alien_azure_clients::models::container_apps::Ingress {
            external: true,
            target_port: Some(8080),
            fqdn: Some(custom_url.to_string()), // Use the full URL as FQDN for the test
            traffic: vec![alien_azure_clients::models::container_apps::TrafficWeight {
                latest_revision: true,
                weight: Some(100),
                revision_name: None,
                label: None,
            }],
            transport: alien_azure_clients::models::container_apps::IngressTransport::Auto,
            allow_insecure: false,
            additional_port_mappings: vec![],
            custom_domains: vec![],
            ip_security_restrictions: vec![],
            cors_policy: None,
            client_certificate_mode: None,
            exposed_port: None,
            sticky_sessions: None,
        });

        ContainerApp {
            id: Some(format!(
                "/subscriptions/12345678-1234-1234-1234-123456789012/resourceGroups/test-rg/providers/Microsoft.App/containerApps/{}",
                app_name
            )),
            name: Some(app_name.to_string()),
            location: "East US".to_string(),
            properties: Some(ContainerAppProperties {
                provisioning_state: Some(ContainerAppPropertiesProvisioningState::Succeeded),
                configuration: Some(Configuration {
                    ingress,
                    active_revisions_mode: ConfigurationActiveRevisionsMode::Single,
                    identity_settings: vec![],
                    registries: vec![],
                    secrets: vec![],
                    dapr: None,
                    max_inactive_revisions: None,
                    runtime: None,
                    service: None,
                }),
                outbound_ip_addresses: vec![],
                custom_domain_verification_id: None,
                environment_id: None,
                event_stream_endpoint: None,
                latest_ready_revision_name: None,
                latest_revision_fqdn: None,
                latest_revision_name: None,
                managed_environment_id: None,
                running_status: None,
                template: None,
                workload_profile_name: None,
            }),
            tags: std::collections::HashMap::new(),
            extended_location: None,
            identity: None,
            managed_by: None,
            system_data: None,
            type_: None,
        }
    }

    // ─────────────── CREATE AND DELETE FLOW TESTS ────────────────────

    #[rstest]
    #[case::basic(basic_function())]
    #[case::env_vars(function_with_env_vars())]
    #[case::storage_link(function_with_storage_link())]
    #[case::env_and_storage(function_with_env_and_storage())]
    #[case::multiple_storages(function_with_multiple_storages())]
    #[case::public_ingress(function_public_ingress())]
    #[case::private_ingress(function_private_ingress())]
    #[case::concurrency(function_with_concurrency())]
    #[case::custom_config(function_custom_config())]
    #[case::readiness_probe(function_with_readiness_probe())]
    #[case::complete_test(function_complete_test())]
    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds(#[case] function: Function) {
        let app_name = format!("test-{}", function.id);
        let (mock_provider, _mock_server) = setup_mocks_for_function(&function, &app_name, true);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify outputs are available
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.identifier.is_some());
        assert!(function_outputs.function_name.starts_with("test-"));

        // Delete the function
        executor.delete().unwrap();

        // Run delete flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── UPDATE FLOW TESTS ────────────────────────────────

    #[rstest]
    #[case::basic_to_env(basic_function(), function_with_env_vars())]
    #[case::env_to_storage(function_with_env_vars(), function_with_storage_link())]
    #[case::storage_to_custom(function_with_storage_link(), function_custom_config())]
    #[case::custom_to_public(function_custom_config(), function_public_ingress())]
    #[case::public_to_complete(function_public_ingress(), function_complete_test())]
    #[case::complete_to_basic(function_complete_test(), basic_function())]
    #[tokio::test]
    async fn test_update_flow_succeeds(
        #[case] from_function: Function,
        #[case] to_function: Function,
    ) {
        // Ensure both functions have the same ID for valid updates
        let function_id = "test-update-function".to_string();
        let mut from_function = from_function;
        from_function.id = function_id.clone();

        let mut to_function = to_function;
        to_function.id = function_id.clone();

        let app_name = format!("test-{}", function_id);
        let (mock_provider, mock_server) = setup_mocks_for_function(&to_function, &app_name, false);

        // Start with the "from" function in Ready state
        let mut ready_controller = AzureFunctionController::mock_ready(&app_name);

        // If the target function has a readiness probe, update the controller URL to point to mock server
        if to_function.readiness_probe.is_some() && to_function.ingress == Ingress::Public {
            if let Some(ref server) = mock_server {
                ready_controller.url = Some(server.base_url());
            }
        } else if to_function.ingress == Ingress::Public {
            // Ensure the controller has a URL for public functions
            ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(from_function)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Update to the new function
        executor.update(to_function).unwrap();

        // Run the update flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    // ─────────────── BEST EFFORT DELETION TESTS ───────────────────────

    #[rstest]
    #[case::basic(basic_function(), false)]
    #[case::public_with_missing_app(function_public_ingress(), true)]
    #[case::private_with_missing_app(function_private_ingress(), true)]
    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing(
        #[case] function: Function,
        #[case] app_missing: bool,
    ) {
        let app_name = format!("test-{}", function.id);
        let mock_container_apps =
            setup_mock_client_for_best_effort_deletion(&app_name, app_missing);
        let mock_provider = setup_mock_service_provider(mock_container_apps, None);

        // Start with a ready controller
        let mut ready_controller = AzureFunctionController::mock_ready(&app_name);
        if function.ingress == Ingress::Public {
            ready_controller.url = Some(format!("https://{}.azurecontainerapps.io", app_name));
        }

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(ready_controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Ensure we start in Running state
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Delete the function
        executor.delete().unwrap();

        // Run the delete flow - it should succeed even when resources are missing
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }

    // ─────────────── LONG RUNNING OPERATION TESTS ──────────────────────

    #[tokio::test]
    async fn test_long_running_creation_operation() {
        let function = basic_function();
        let app_name = format!("test-{}", function.id);
        let (mock_container_apps, mock_lro) =
            setup_mock_client_for_long_running_creation(&app_name, false);
        let mock_provider = setup_mock_service_provider(mock_container_apps, Some(mock_lro));

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Run create flow
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify the controller went through LRO states
        let controller = executor
            .internal_state::<AzureFunctionController>()
            .unwrap();
        assert!(controller.container_app_name.is_some());
        assert!(controller.resource_id.is_some());
    }

    // ─────────────── SPECIFIC VALIDATION TESTS ─────────────────

    /// Test that verifies public functions get URL in outputs
    #[tokio::test]
    async fn test_public_function_gets_url_in_outputs() {
        let function = function_public_ingress();
        let app_name = format!("test-{}", function.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock creation with URL
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name, true),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify URL is in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_some());
        assert!(function_outputs
            .url
            .as_ref()
            .unwrap()
            .contains("azurecontainerapps.io"));
    }

    /// Test that verifies private functions don't get URL in outputs
    #[tokio::test]
    async fn test_private_function_has_no_url_in_outputs() {
        let function = function_private_ingress();
        let app_name = format!("test-{}", function.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Mock creation without URL
        mock_container_apps
            .expect_create_or_update_container_app()
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            });

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        // Verify no URL in outputs
        let outputs = executor.outputs().unwrap();
        let function_outputs = outputs.downcast_ref::<FunctionOutputs>().unwrap();
        assert!(function_outputs.url.is_none());
    }

    /// Test that verifies correct container app configuration parameters
    #[tokio::test]
    async fn test_container_app_configuration_validation() {
        let function = function_custom_config();
        let app_name = format!("test-{}", function.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Validate container app creation request has correct parameters
        let app_name_for_response = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .withf(|_rg, _name, container_app| {
                // Check that the container has correct resource configuration
                if let Some(properties) = &container_app.properties {
                    if let Some(template) = &properties.template {
                        if let Some(container) = template.containers.first() {
                            if let Some(resources) = &container.resources {
                                // function_custom_config has 512MB memory
                                let expected_memory = format!("{}Gi", 512.0 / 1024.0);
                                return resources.memory.as_ref() == Some(&expected_memory)
                                    && resources.cpu == Some(0.25);
                            }
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_response, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Allow get_container_app calls during creation (may be called 0 or more times)
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
            .times(0..);

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies environment variables are correctly passed
    #[tokio::test]
    async fn test_environment_variable_handling() {
        let function = function_with_env_vars();
        let app_name = format!("test-{}", function.id);

        let mut mock_container_apps = MockContainerAppsApi::new();

        // Validate container app creation request has environment variables
        let app_name_for_response = app_name.clone();
        mock_container_apps
            .expect_create_or_update_container_app()
            .withf(|_rg, _name, container_app| {
                if let Some(properties) = &container_app.properties {
                    if let Some(template) = &properties.template {
                        if let Some(container) = template.containers.first() {
                            // Check that environment variables are present
                            let has_app_env = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("APP_ENV")
                                    && env_var.value.as_deref() == Some("production")
                            });
                            let has_log_level = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("LOG_LEVEL")
                                    && env_var.value.as_deref() == Some("debug")
                            });
                            let has_db_name = container.env.iter().any(|env_var| {
                                env_var.name.as_deref() == Some("DB_NAME")
                                    && env_var.value.as_deref() == Some("myapp")
                            });
                            return has_app_env && has_log_level && has_db_name;
                        }
                    }
                }
                false
            })
            .returning(move |_, _, _| {
                Ok(OperationResult::Completed(
                    create_successful_container_app_response(&app_name_for_response, false),
                ))
            });

        mock_container_apps
            .expect_delete_container_app()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        // Allow get_container_app calls during creation (may be called 0 or more times)
        mock_container_apps
            .expect_get_container_app()
            .returning(move |_, _| Ok(create_successful_container_app_response(&app_name, false)))
            .times(0..);

        // Mock get operation failure for deletion verification
        mock_container_apps
            .expect_get_container_app()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ContainerApp".to_string(),
                        resource_name: "test-app".to_string(),
                    },
                ))
            })
            .times(0..);

        let mock_provider = setup_mock_service_provider(Arc::new(mock_container_apps), None);

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(AzureFunctionController::default())
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    /// Test that verifies deletion works when container_app_name is not set (early creation failure)
    #[tokio::test]
    async fn test_delete_with_no_container_app_name_succeeds() {
        let function = basic_function();

        // Create a controller with no container app name set (simulating early creation failure)
        let controller = AzureFunctionController {
            state: AzureFunctionState::CreateFailed,
            container_app_name: None, // This is the key - no container app name set
            resource_id: None,
            url: None,
            pending_operation_url: None,
            pending_operation_retry_after: None,
            dapr_components: Vec::new(),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            commands_namespace_name: None,
            commands_queue_name: None,
            commands_dapr_component: None,
            _internal_stay_count: None,
        };

        // Mock provider - no expectations since no API calls should be made
        let mock_provider = Arc::new(MockPlatformServiceProvider::new());

        let mut executor = SingleControllerExecutor::builder()
            .resource(function)
            .controller(controller)
            .platform(Platform::Azure)
            .service_provider(mock_provider)
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        // Start in CreateFailed state
        assert_eq!(executor.status(), ResourceStatus::ProvisionFailed);

        // Delete the function
        executor.delete().unwrap();

        // Run the delete flow - should succeed without making any API calls
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);

        // Verify outputs are no longer available
        assert!(executor.outputs().is_none());
    }
}
