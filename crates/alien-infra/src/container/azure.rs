//! Azure Container Controller
//!
//! This module implements the Azure-specific controller for managing Container resources.
//! A Container represents a deployable workload that runs on a ContainerCluster.
//!
//! The controller:
//! - Creates Azure Load Balancer resources for public containers
//! - Creates Managed Disks for persistent storage
//! - Calls Horizon API to create/update/delete containers
//! - Monitors container status via Horizon
//!
//! Container scheduling and replica management is handled by Horizon, not this controller.

use alien_azure_clients::azure::disks::ManagedDisksApi;
use alien_azure_clients::azure::load_balancers::LoadBalancerApi;
use alien_azure_clients::azure::models::disk_rp::{
    CreationData, Disk, DiskCreateOption, DiskProperties, DiskSku, DiskStorageAccountTypes,
};
use alien_azure_clients::azure::models::load_balancer::{
    BackendAddressPool, BackendAddressPoolPropertiesFormat, FrontendIpConfiguration,
    FrontendIpConfigurationPropertiesFormat, LoadBalancer, LoadBalancerPropertiesFormat,
    LoadBalancerSku, LoadBalancerSkuName, LoadBalancerSkuTier, LoadBalancingRule,
    LoadBalancingRulePropertiesFormat, Probe, ProbePropertiesFormat, ProbePropertiesFormatProtocol,
    SubResource, TransportProtocol,
};
use alien_azure_clients::azure::models::public_ip_address::{
    IpAllocationMethod, PublicIpAddress, PublicIpAddressPropertiesFormat, PublicIpAddressSku,
    PublicIpAddressSkuName, PublicIpAddressSkuTier,
};
use alien_azure_clients::azure::network::NetworkApi;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CertificateStatus, Container, ContainerCluster, ContainerCode, ContainerOutputs,
    ContainerStatus, DnsRecordStatus, ExposeProtocol, HorizonClusterConfig, Network,
    ResourceOutputs, ResourceRef, ResourceStatus,
};
use alien_error::{AlienError, Context as ContextError, IntoAlienError};
use alien_macros::{controller, flow_entry, handler, terminal_state};
use base64;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::num::NonZeroU64;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::container_cluster::AzureContainerClusterController;
use crate::core::{environment_variables::EnvironmentVariableBuilder, ResourceControllerContext};
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, horizon_container_status_to_alien};
use crate::infra_requirements::azure_utils;
use crate::network::AzureNetworkController;

/// Tracks a Managed Disk created for a stateful container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedDiskState {
    /// The disk ID (full ARM resource ID)
    pub disk_id: String,
    /// The disk name
    pub disk_name: String,
    /// The Azure zone (e.g., "1")
    pub zone: String,
    /// The ordinal this disk is for (for stateful containers)
    pub ordinal: u32,
    /// Size in GB
    pub size_gb: u32,
}

/// Convert a PKCS#8 PEM private key + PEM certificate chain into a PKCS#12 (PFX) blob.
///
/// Requirements:
/// - `private_key_pem` contains an *unencrypted* PKCS#8 key: `-----BEGIN PRIVATE KEY-----`
/// - `certificate_chain_pem` contains leaf cert first, then intermediates (if any)
///
/// Azure Key Vault requires PKCS#12 format. We use empty password (Azure handles encryption).
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
        "Alien Container Certificate",
    )
    .ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: "Failed to build PKCS#12 (p12::PFX::new_with_cas returned None)".to_string(),
            resource_id: None,
        })
    })?;

    Ok(pfx.to_der())
}

/// Azure Container Controller state machine.
///
/// This controller manages the lifecycle of containers via Horizon:
/// - Creates Azure Load Balancer resources for public containers
/// - Creates Managed Disks for persistent storage
/// - Creates containers in Horizon when the ContainerCluster is ready
/// - Updates container configuration via Horizon API
/// - Deletes containers from Horizon during cleanup
#[controller]
pub struct AzureContainerController {
    /// Horizon container name (derived from resource ID)
    pub(crate) container_name: Option<String>,

    /// Current status from Horizon
    pub(crate) horizon_status: Option<ContainerStatus>,

    /// Number of running replicas
    pub(crate) current_replicas: u32,

    /// Public URL (Load Balancer IP if exposed publicly)
    pub(crate) public_url: Option<String>,

    /// Public IP name (if exposed publicly)
    pub(crate) public_ip_name: Option<String>,

    /// Public IP resource ID (if exposed publicly)
    pub(crate) public_ip_id: Option<String>,

    /// Load Balancer name (if exposed publicly)
    pub(crate) load_balancer_name: Option<String>,

    /// Backend pool name (if exposed publicly)
    pub(crate) backend_pool_name: Option<String>,

    /// Frontend IP configuration name (if exposed publicly)
    pub(crate) frontend_ip_config_name: Option<String>,

    /// Health probe name (if exposed publicly)
    pub(crate) probe_name: Option<String>,

    /// Load balancing rule name (if exposed publicly)
    pub(crate) lb_rule_name: Option<String>,

    /// Fully qualified domain name (custom or generated)
    pub(crate) fqdn: Option<String>,

    /// Certificate ID for auto-managed domains
    pub(crate) certificate_id: Option<String>,

    /// Key Vault certificate identifier (URL)
    pub(crate) keyvault_cert_id: Option<String>,

    /// Whether this resource uses a customer-managed domain
    pub(crate) uses_custom_domain: bool,

    /// Timestamp when certificate was imported (for renewal detection)
    pub(crate) certificate_issued_at: Option<String>,

    /// Managed Disks created for persistent storage
    pub(crate) managed_disks: Vec<ManagedDiskState>,

    /// Number of iterations spent waiting for replicas to become healthy
    #[serde(default)]
    pub(crate) wait_for_replicas_iterations: u32,
}

/// Context for interacting with Horizon API for a specific cluster.
struct HorizonContext<'a> {
    /// Cluster configuration (contains cluster_id)
    cluster: &'a HorizonClusterConfig,
    /// Pre-authenticated Horizon client
    client: horizon_client_sdk::Client,
}

struct DomainInfo {
    fqdn: String,
    certificate_id: Option<String>,
    keyvault_cert_id: Option<String>,
    uses_custom_domain: bool,
}

impl AzureContainerController {
    /// Resolve domain information for a public container.
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

    /// Get Horizon context for the given cluster.
    /// Returns the cluster config and an authenticated client.
    fn horizon<'a>(
        ctx: &'a ResourceControllerContext<'_>,
        cluster_resource_id: &str,
    ) -> Result<HorizonContext<'a>> {
        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(alien_core::ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container resources require a Horizon compute backend".to_string(),
                    resource_id: Some(cluster_resource_id.to_string()),
                }))
            }
        };

        let cluster = horizon_config
            .clusters
            .get(cluster_resource_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("No Horizon cluster config for '{}'", cluster_resource_id),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        let client = create_horizon_client(&horizon_config.url, &cluster.management_token)
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Horizon client: {}", e),
                    resource_id: Some(cluster_resource_id.to_string()),
                })
            })?;

        Ok(HorizonContext { cluster, client })
    }

    /// Parse storage size string (e.g., "100Gi", "500GB") to GB.
    fn parse_storage_size_gb(size: &str) -> Result<u32> {
        let size = size.trim();
        let (num_str, unit) = if size.ends_with("Gi") || size.ends_with("GiB") {
            (size.trim_end_matches("GiB").trim_end_matches("Gi"), "Gi")
        } else if size.ends_with("GB") {
            (size.trim_end_matches("GB"), "GB")
        } else if size.ends_with("Ti") || size.ends_with("TiB") {
            (size.trim_end_matches("TiB").trim_end_matches("Ti"), "Ti")
        } else if size.ends_with("TB") {
            (size.trim_end_matches("TB"), "TB")
        } else {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!(
                    "Invalid storage size format: {}. Expected format like '100Gi' or '500GB'",
                    size
                ),
                resource_id: None,
            }));
        };

        let num: u32 = num_str.parse().map_err(|_| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Invalid storage size number: {}", num_str),
                resource_id: None,
            })
        })?;

        let gb = match unit {
            "Gi" | "GiB" => num,
            "GB" => num,
            "Ti" | "TiB" => num * 1024,
            "TB" => num * 1000,
            _ => num,
        };

        Ok(gb)
    }
}

#[controller]
impl AzureContainerController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(container_id = %config.id, cluster = %cluster, "Starting Container provisioning");

        self.container_name = Some(config.id.clone());

        // Determine next step based on what infrastructure we need to create
        let exposed_port = config.ports.iter().find(|p| p.expose.is_some());

        if let Some(port_config) = exposed_port {
            let is_http = matches!(port_config.expose.as_ref().unwrap(), ExposeProtocol::Http);
            // Resolve domain information
            let domain_info = Self::resolve_domain_info(ctx, &config.id)?;
            self.fqdn = Some(domain_info.fqdn.clone());
            self.certificate_id = domain_info.certificate_id;
            self.keyvault_cert_id = domain_info.keyvault_cert_id;
            self.uses_custom_domain = domain_info.uses_custom_domain;

            // Check for URL override in deployment config, otherwise use domain FQDN
            self.public_url = ctx
                .deployment_config
                .public_urls
                .as_ref()
                .and_then(|urls| urls.get(&config.id).cloned())
                .or_else(|| Some(format!("https://{}", domain_info.fqdn)));

            // If using auto-managed domain, wait for certificate first
            if !self.uses_custom_domain {
                Ok(HandlerAction::Continue {
                    state: WaitingForCertificate,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            } else {
                Ok(HandlerAction::Continue {
                    state: CreatingPublicIp,
                    suggested_delay: None,
                })
            }
        } else if config.persistent_storage.is_some() {
            Ok(HandlerAction::Continue {
                state: CreatingManagedDisks,
                suggested_delay: None,
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
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
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.certificate_status);

        match status {
            Some(CertificateStatus::Issued) => {
                info!(container_id = %config.id, "Certificate issued, proceeding to import");
                Ok(HandlerAction::Continue {
                    state: ImportingCertificate,
                    suggested_delay: None,
                })
            }
            Some(CertificateStatus::Failed) => {
                Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: "Certificate issuance failed".to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => {
                debug!(container_id = %config.id, "Certificate not yet issued, waiting");
                Ok(HandlerAction::Stay {
                    max_times: 60,
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
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
        let config = ctx.desired_resource_config::<Container>()?;

        let resource = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id))
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Domain metadata missing for certificate import".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        // Certificate data is included in DeploymentConfig
        let certificate_chain = resource.certificate_chain.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Certificate chain missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let private_key = resource.private_key.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Private key missing (certificate not issued)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Convert PEM to PKCS12 format for Azure Key Vault
        let pkcs12_data = pem_to_pkcs12(private_key, certificate_chain)?;

        use base64::Engine;
        let pkcs12_base64 = base64::engine::general_purpose::STANDARD.encode(&pkcs12_data);

        // Get Key Vault details from Azure config
        // For now, we'll need to create a Key Vault or use an existing one
        // This would typically come from the Azure infrastructure setup
        // Let's use the resource group name to construct the vault name
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;

        // Construct vault name (Azure Key Vault names must be globally unique, 3-24 chars, alphanumeric and hyphens)
        let vault_name = format!("{}-kv", ctx.resource_prefix.replace("_", "-"));
        let vault_url = format!("https://{}.vault.azure.net", vault_name);
        let cert_name = format!("{}-{}-cert", ctx.resource_prefix, config.id).replace("_", "-");

        info!(
            container_id = %config.id,
            vault_name = %vault_name,
            cert_name = %cert_name,
            "Importing certificate to Azure Key Vault"
        );

        use alien_azure_clients::azure::keyvault::{
            AzureKeyVaultCertificatesClient, KeyVaultCertificatesApi,
        };
        use alien_azure_clients::azure::models::certificates::CertificateImportParameters;

        let kv_client = ctx
            .service_provider
            .get_azure_key_vault_certificates_client(azure_cfg)?;

        let import_params = CertificateImportParameters {
            value: pkcs12_base64,
            pwd: None, // No password on the PKCS12
            policy: None,
            attributes: None,
            tags: std::collections::HashMap::new(),
            preserve_cert_order: None,
        };

        let cert_bundle = kv_client
            .import_certificate(vault_url.clone(), cert_name.clone(), import_params)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to import certificate to Azure Key Vault".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Store the certificate ID (URL) for later use
        self.keyvault_cert_id = cert_bundle.id.clone();

        // Store issued_at timestamp for renewal detection
        self.certificate_issued_at = resource.issued_at.clone();

        info!(
            container_id = %config.id,
            cert_id = ?self.keyvault_cert_id,
            "Certificate imported to Azure Key Vault"
        );

        Ok(HandlerAction::Continue {
            state: CreatingPublicIp,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingPublicIp,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_public_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let network_client = ctx.service_provider.get_azure_network_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let public_ip_name = format!("{}-{}-pip", ctx.resource_prefix, config.id);

        info!(public_ip_name = %public_ip_name, "Creating public IP for container");

        let public_ip = PublicIpAddress {
            location: azure_cfg.region.clone(),
            sku: Some(PublicIpAddressSku {
                name: Some(PublicIpAddressSkuName::Standard),
                tier: Some(PublicIpAddressSkuTier::Regional),
            }),
            properties: Some(Box::new(PublicIpAddressPropertiesFormat {
                public_ip_allocation_method: Some(IpAllocationMethod::Static),
                ..Default::default()
            })),
            ..Default::default()
        };

        let op_result = network_client
            .create_or_update_public_ip_address(&resource_group_name, &public_ip_name, &public_ip)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create public IP address".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        op_result
            .wait_for_operation_completion(&*lro_client, "CreatePublicIpAddress", &public_ip_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed while waiting for public IP creation".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let created_public_ip = network_client
            .get_public_ip_address(&resource_group_name, &public_ip_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read public IP address".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.public_ip_id = created_public_ip.id.clone();
        self.public_ip_name = Some(public_ip_name);
        self.public_url = created_public_ip
            .properties
            .as_ref()
            .and_then(|props| props.ip_address.clone())
            .map(|ip| format!("http://{}", ip));

        Ok(HandlerAction::Continue {
            state: CreatingLoadBalancer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingLoadBalancer,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_load_balancer(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let lb_client = ctx
            .service_provider
            .get_azure_load_balancer_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<Container>()?;

        let public_ip_id = self.public_ip_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Public IP ID not set for load balancer".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let load_balancer_name = format!("{}-{}-lb", ctx.resource_prefix, config.id);
        let frontend_ip_config_name = format!("{}-{}-frontend", ctx.resource_prefix, config.id);
        let backend_pool_name = format!("{}-{}-pool", ctx.resource_prefix, config.id);
        let probe_name = format!("{}-{}-probe", ctx.resource_prefix, config.id);
        let lb_rule_name = format!("{}-{}-rule", ctx.resource_prefix, config.id);

        let frontend_ip_config_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/frontendIPConfigurations/{}",
            azure_cfg.subscription_id, resource_group_name, load_balancer_name, frontend_ip_config_name
        );
        let backend_pool_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/backendAddressPools/{}",
            azure_cfg.subscription_id, resource_group_name, load_balancer_name, backend_pool_name
        );
        let probe_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/loadBalancers/{}/probes/{}",
            azure_cfg.subscription_id, resource_group_name, load_balancer_name, probe_name
        );

        let load_balancer = LoadBalancer {
            location: azure_cfg.region.clone(),
            sku: Some(LoadBalancerSku {
                name: Some(LoadBalancerSkuName::Standard),
                tier: Some(LoadBalancerSkuTier::Regional),
            }),
            properties: Some(LoadBalancerPropertiesFormat {
                frontend_ip_configurations: vec![FrontendIpConfiguration {
                    name: Some(frontend_ip_config_name.clone()),
                    properties: Some(FrontendIpConfigurationPropertiesFormat {
                        public_ip_address: Some(
                            alien_azure_clients::azure::models::load_balancer::PublicIpAddress {
                                id: Some(public_ip_id.clone()),
                                ..Default::default()
                            },
                        ),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                backend_address_pools: vec![BackendAddressPool {
                    name: Some(backend_pool_name.clone()),
                    properties: Some(BackendAddressPoolPropertiesFormat {
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                probes: vec![Probe {
                    name: Some(probe_name.clone()),
                    properties: Some(ProbePropertiesFormat {
                        protocol: ProbePropertiesFormatProtocol::Tcp,
                        port: config.ports.first().map(|p| p.port as i32).unwrap_or(8080),
                        interval_in_seconds: Some(15),
                        number_of_probes: Some(2),
                        probe_threshold: Some(1),
                        request_path: None,
                        load_balancing_rules: vec![],
                        no_healthy_backends_behavior: None,
                        provisioning_state: None,
                    }),
                    ..Default::default()
                }],
                load_balancing_rules: vec![LoadBalancingRule {
                    name: Some(lb_rule_name.clone()),
                    properties: Some(LoadBalancingRulePropertiesFormat {
                        frontend_ip_configuration: Some(SubResource {
                            id: Some(frontend_ip_config_id),
                        }),
                        backend_address_pool: Some(SubResource {
                            id: Some(backend_pool_id),
                        }),
                        backend_address_pools: vec![],
                        probe: Some(SubResource { id: Some(probe_id) }),
                        protocol: TransportProtocol::Tcp,
                        frontend_port: config.ports.first().map(|p| p.port as i32).unwrap_or(8080),
                        backend_port: Some(
                            config.ports.first().map(|p| p.port as i32).unwrap_or(8080),
                        ),
                        load_distribution: None,
                        idle_timeout_in_minutes: Some(4),
                        enable_floating_ip: Some(false),
                        enable_tcp_reset: Some(true),
                        enable_connection_tracking: None,
                        disable_outbound_snat: Some(false),
                        provisioning_state: None,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let op_result = lb_client
            .create_or_update_load_balancer(
                &resource_group_name,
                &load_balancer_name,
                &load_balancer,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create load balancer".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        op_result
            .wait_for_operation_completion(&*lro_client, "CreateLoadBalancer", &load_balancer_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed while waiting for load balancer creation".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.load_balancer_name = Some(load_balancer_name);
        self.backend_pool_name = Some(backend_pool_name);
        self.frontend_ip_config_name = Some(frontend_ip_config_name);
        self.probe_name = Some(probe_name);
        self.lb_rule_name = Some(lb_rule_name);

        Ok(HandlerAction::Continue {
            state: CreatingManagedDisks,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingManagedDisks,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_managed_disks(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let storage_size = match &config.persistent_storage {
            Some(size) => Self::parse_storage_size_gb(&size.size)?,
            None => {
                // No persistent storage, check if we need to wait for DNS
                if self.fqdn.is_some() && !self.uses_custom_domain {
                    return Ok(HandlerAction::Continue {
                        state: WaitingForDns,
                        suggested_delay: Some(Duration::from_secs(5)),
                    });
                } else {
                    return Ok(HandlerAction::Continue {
                        state: CreatingHorizonContainer,
                        suggested_delay: None,
                    });
                }
            }
        };

        let replicas = config.replicas.ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Stateful containers with persistent storage must specify replicas"
                    .to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let disk_client = ctx
            .service_provider
            .get_azure_managed_disks_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        let zone = "1".to_string();

        info!(
            container_id = %config.id,
            replicas = replicas,
            size_gb = storage_size,
            "Creating managed disks for stateful container"
        );

        for ordinal in 0..replicas {
            let disk_name = format!("{}-{}-disk-{}", ctx.resource_prefix, config.id, ordinal);
            let disk = Disk {
                location: azure_cfg
                    .region
                    .clone()
                    .unwrap_or_else(|| "eastus".to_string()),
                sku: Some(DiskSku {
                    name: Some(DiskStorageAccountTypes::StandardSsdLrs),
                    tier: None,
                }),
                properties: Some(DiskProperties {
                    creation_data: CreationData {
                        create_option: DiskCreateOption::Empty,
                        elastic_san_resource_id: None,
                        gallery_image_reference: None,
                        image_reference: None,
                        instant_access_duration_minutes: None,
                        logical_sector_size: None,
                        performance_plus: None,
                        provisioned_bandwidth_copy_speed: None,
                        security_data_uri: None,
                        security_metadata_uri: None,
                        source_resource_id: None,
                        source_unique_id: None,
                        source_uri: None,
                        storage_account_id: None,
                        upload_size_bytes: None,
                    },
                    disk_size_gb: Some(storage_size as i32),
                    disk_iops_read_write: None,
                    disk_m_bps_read_write: None,
                    disk_iops_read_only: None,
                    disk_m_bps_read_only: None,
                    availability_policy: None,
                    bursting_enabled: None,
                    bursting_enabled_time: None,
                    completion_percent: None,
                    data_access_auth_mode: None,
                    disk_access_id: None,
                    disk_size_bytes: None,
                    disk_state: None,
                    encryption: None,
                    encryption_settings_collection: None,
                    hyper_v_generation: None,
                    last_ownership_update_time: None,
                    max_shares: None,
                    network_access_policy: None,
                    optimized_for_frequent_attach: None,
                    os_type: None,
                    property_updates_in_progress: None,
                    provisioning_state: None,
                    public_network_access: None,
                    purchase_plan: None,
                    security_profile: None,
                    share_info: vec![],
                    supported_capabilities: None,
                    supports_hibernation: None,
                    tier: None,
                    time_created: None,
                    unique_id: None,
                }),
                tags: HashMap::new(),
                id: None,
                name: None,
                type_: None,
                managed_by: None,
                managed_by_extended: vec![],
                extended_location: None,
                zones: vec![zone.clone()],
                system_data: None,
            };

            let op_result = disk_client
                .create_or_update_disk(&resource_group_name, &disk_name, &disk)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create managed disk {}", disk_name),
                    resource_id: Some(config.id.clone()),
                })?;

            op_result
                .wait_for_operation_completion(&*lro_client, "CreateManagedDisk", &disk_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed while waiting for disk {}", disk_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let created_disk = disk_client
                .get_disk(&resource_group_name, &disk_name)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read disk {}", disk_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let disk_id = created_disk.id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Disk {} missing ID", disk_name),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            self.managed_disks.push(ManagedDiskState {
                disk_id,
                disk_name,
                zone: zone.clone(),
                ordinal,
                size_gb: storage_size,
            });
        }

        // After creating disks, check if we need to wait for DNS
        if self.fqdn.is_some() && !self.uses_custom_domain {
            Ok(HandlerAction::Continue {
                state: WaitingForDns,
                suggested_delay: Some(Duration::from_secs(5)),
            })
        } else {
            Ok(HandlerAction::Continue {
                state: CreatingHorizonContainer,
                suggested_delay: None,
            })
        }
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
        let config = ctx.desired_resource_config::<Container>()?;
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|meta| meta.resources.get(&config.id));

        let status = metadata.map(|m| &m.dns_status);

        match status {
            Some(DnsRecordStatus::Active) => {
                info!(container_id = %config.id, "DNS record active, proceeding");
                Ok(HandlerAction::Continue {
                    state: CreatingHorizonContainer,
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
                    resource_id: Some(config.id.clone()),
                }))
            }
            _ => {
                debug!(container_id = %config.id, "DNS record not yet active, waiting");
                Ok(HandlerAction::Stay {
                    max_times: 60, // 5 minutes max
                    suggested_delay: Some(Duration::from_secs(5)),
                })
            }
        }
    }

    #[handler(
        state = CreatingHorizonContainer,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cluster_ref = ResourceRef::new(ContainerCluster::RESOURCE_TYPE, cluster_id.clone());
        let _cluster = ctx.require_dependency::<AzureContainerClusterController>(&cluster_ref)?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            load_balancer = ?self.load_balancer_name,
            disk_count = self.managed_disks.len(),
            "Creating container in Horizon"
        );

        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let cpu: horizon_client_sdk::types::ResourceRequirementsCpu =
            horizon_client_sdk::types::ResourceRequirementsCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::ResourceRequirementsMemory =
            horizon_client_sdk::types::ResourceRequirementsMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder = horizon_client_sdk::types::ResourceRequirements::builder()
            .cpu(cpu)
            .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::ResourceRequirementsEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid ephemeral storage config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::ResourceRequirements =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let ports: Vec<NonZeroU64> = config
            .ports
            .iter()
            .filter_map(|p| NonZeroU64::new(p.port as u64))
            .collect();

        let capacity_group = config.pool.clone().unwrap_or_else(|| "general".to_string());

        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        let mut request_builder = horizon_client_sdk::types::CreateContainerRequest::builder()
            .name(&config.id)
            .capacity_group(&capacity_group)
            .image(&image)
            .resources(resources)
            .stateful(config.stateful)
            .ports(ports)
            .env(env_vars);

        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::CreateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }

            let autoscaling_config: horizon_client_sdk::types::CreateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        // Wire per-container cloud identity so horizond can vend credentials via the IMDS proxy.
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                if let Some(client_id) = &sa_ctrl.identity_client_id {
                    let sa = horizon_client_sdk::types::ServiceAccountTarget::from(
                        horizon_client_sdk::types::AzureServiceAccountTarget {
                            client_id: client_id.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!(
                                        "Invalid managed identity client ID '{}'",
                                        client_id
                                    ),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            type_: horizon_client_sdk::types::AzureServiceAccountTargetType::Azure,
                        },
                    );
                    request_builder = request_builder.service_account(sa);
                    info!(
                        container_id = %config.id,
                        client_id = %client_id,
                        "Wired Azure managed identity to container for IMDS credential vending"
                    );
                }
            }
        }

        if let (Some(load_balancer_name), Some(backend_pool_name)) =
            (&self.load_balancer_name, &self.backend_pool_name)
        {
            let network_ref =
                ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
            let network = ctx.require_dependency::<AzureNetworkController>(&network_ref)?;
            let vnet_id = network.vnet_resource_id.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "VNet resource ID not available".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            let lb_target = horizon_client_sdk::types::LoadBalancerTarget::Azure {
                resource_group: resource_group_name.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid resource group '{}'", resource_group_name),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
                load_balancer_name: load_balancer_name.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid load balancer name '{}'", load_balancer_name),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
                backend_pool_name: backend_pool_name.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid backend pool name '{}'", backend_pool_name),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
                virtual_network_id: vnet_id.parse().into_alien_error().context(
                    ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid virtual network ID '{}'", vnet_id),
                        resource_id: Some(config.id.clone()),
                    },
                )?,
            };
            request_builder = request_builder.load_balancer_target(lb_target);
        }

        if !self.managed_disks.is_empty() {
            let volumes = self
                .managed_disks
                .iter()
                .map(|v| {
                    Ok::<_, AlienError<ErrorData>>(horizon_client_sdk::types::VolumeRegistration {
                        ordinal: v.ordinal as u64,
                        volume: horizon_client_sdk::types::VolumeTarget::Azure {
                            disk_id: v.disk_id.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid disk ID '{}'", v.disk_id),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            zone: v.zone.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!("Invalid zone '{}'", v.zone),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                        },
                    })
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            request_builder = request_builder.volumes(volumes);
        }

        let request: horizon_client_sdk::types::CreateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build container request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let response = horizon
            .client
            .create_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(response.status));

        info!(
            container_id = %config.id,
            "Container created in Horizon, waiting for replicas"
        );

        Ok(HandlerAction::Continue {
            state: WaitingForReplicas,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForReplicas,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_replicas(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        let container = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let healthy_replicas = container.replicas_info.iter().filter(|r| r.healthy).count() as u32;

        let desired = config
            .replicas
            .or(config.autoscaling.as_ref().map(|a| a.desired))
            .unwrap_or(1);

        self.horizon_status = Some(horizon_container_status_to_alien(container.status));
        self.current_replicas = healthy_replicas;

        debug!(
            container_id = %config.id,
            healthy = healthy_replicas,
            desired = desired,
            "Container replica status"
        );

        if healthy_replicas >= desired.min(1) {
            info!(
                container_id = %config.id,
                healthy_replicas = healthy_replicas,
                "Container replicas are healthy"
            );

            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else {
            self.wait_for_replicas_iterations += 1;
            if self.wait_for_replicas_iterations >= 30 {
                // If the parent cluster is mid-update/provision, replica disruption is expected:
                // the rolling update will bring fresh VMs with updated horizond. Reset and wait.
                let cluster_is_updating = ctx.state.resources.get(cluster_id).map_or(false, |s| {
                    matches!(
                        s.status,
                        ResourceStatus::Updating | ResourceStatus::Provisioning
                    )
                });
                if cluster_is_updating {
                    info!(
                        container_id = %config.id,
                        cluster_id = %cluster_id,
                        "Parent cluster is updating, resetting health check counter"
                    );
                    self.wait_for_replicas_iterations = 0;
                } else {
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Container replicas did not become healthy after 30 iterations (~5 min). \
                             Last Horizon status: {:?}, healthy replicas: {}/{}",
                            self.horizon_status, self.current_replicas, desired
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
            }
            debug!(
                container_id = %config.id,
                healthy_replicas = healthy_replicas,
                iteration = self.wait_for_replicas_iterations,
                "Waiting for container replicas to become healthy"
            );
            Ok(HandlerAction::Stay {
                max_times: 35, // safety backstop; manual check above fires first
                suggested_delay: Some(Duration::from_secs(10)),
            })
        }
    }

    // ─────────────── READY STATE ────────────────────────────────

    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running,
    )]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        debug!(container_id = %config.id, "Container ready, checking health");

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        let container = horizon
            .client
            .get_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to get container status from Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.horizon_status = Some(horizon_container_status_to_alien(container.status));
        self.current_replicas = container.replicas_info.iter().filter(|r| r.healthy).count() as u32;

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
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Container update requested");

        Ok(HandlerAction::Continue {
            state: UpdatingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingHorizonContainer,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            "Updating container in Horizon"
        );

        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Container is configured with source code, but only pre-built images are supported".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };

        let image_typed: horizon_client_sdk::types::UpdateContainerRequestImage =
            image.as_str().try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid image: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let env_vars = EnvironmentVariableBuilder::new(&config.environment)
            .add_standard_alien_env_vars(ctx)
            .add_container_transport_env_vars()
            .add_linked_resources(&config.links, ctx, &config.id)
            .await?
            .build();

        let cpu: horizon_client_sdk::types::UpdateContainerRequestResourcesCpu =
            horizon_client_sdk::types::UpdateContainerRequestResourcesCpu::builder()
                .min(&config.cpu.min)
                .desired(&config.cpu.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid CPU config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let memory: horizon_client_sdk::types::UpdateContainerRequestResourcesMemory =
            horizon_client_sdk::types::UpdateContainerRequestResourcesMemory::builder()
                .min(&config.memory.min)
                .desired(&config.memory.desired)
                .try_into()
                .map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid memory config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

        let mut resources_builder =
            horizon_client_sdk::types::UpdateContainerRequestResources::builder()
                .cpu(cpu)
                .memory(memory);

        if let Some(ephemeral) = &config.ephemeral_storage {
            let ephemeral_storage: horizon_client_sdk::types::UpdateContainerRequestResourcesEphemeralStorage =
                ephemeral.as_str().try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid ephemeral storage config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            resources_builder = resources_builder.ephemeral_storage(ephemeral_storage);
        }

        if let Some(gpu) = &config.gpu {
            let gpu_spec: horizon_client_sdk::types::GpuSpec =
                horizon_client_sdk::types::GpuSpec::builder()
                    .type_(gpu.gpu_type.clone())
                    .count(NonZeroU64::new(gpu.count as u64).unwrap_or(NonZeroU64::new(1).unwrap()))
                    .try_into()
                    .map_err(|e| {
                        AlienError::new(ErrorData::ResourceConfigInvalid {
                            message: format!("Invalid GPU config: {:?}", e),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
            resources_builder = resources_builder.gpu(gpu_spec);
        }

        let resources: horizon_client_sdk::types::UpdateContainerRequestResources =
            resources_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Invalid resources config: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let mut request_builder = horizon_client_sdk::types::UpdateContainerRequest::builder()
            .image(image_typed)
            .env(env_vars)
            .resources(resources);

        if let Some(cmd) = &config.command {
            request_builder = request_builder.command(cmd.clone());
        }

        if config.stateful {
            if let Some(replicas) = config.replicas {
                if let Some(nz) = NonZeroU64::new(replicas as u64) {
                    request_builder = request_builder.replicas(nz);
                }
            }
        } else if let Some(autoscaling) = &config.autoscaling {
            let mut autoscaling_builder =
                horizon_client_sdk::types::UpdateContainerRequestAutoscaling::builder()
                    .min(
                        NonZeroU64::new(autoscaling.min as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .desired(
                        NonZeroU64::new(autoscaling.desired as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    )
                    .max(
                        NonZeroU64::new(autoscaling.max as u64)
                            .unwrap_or(NonZeroU64::new(1).unwrap()),
                    );

            if let Some(cpu_pct) = autoscaling.target_cpu_percent {
                autoscaling_builder = autoscaling_builder.target_cpu_percent(cpu_pct as f64);
            }
            if let Some(mem_pct) = autoscaling.target_memory_percent {
                autoscaling_builder = autoscaling_builder.target_memory_percent(mem_pct as f64);
            }
            if let Some(http_inflight) = autoscaling.target_http_in_flight_per_replica {
                if let Some(nz) = NonZeroU64::new(http_inflight as u64) {
                    autoscaling_builder = autoscaling_builder.target_http_in_flight_per_replica(nz);
                }
            }
            if let Some(p95_latency) = autoscaling.max_http_p95_latency_ms {
                autoscaling_builder = autoscaling_builder.max_http_p95_latency_ms(p95_latency);
            }

            let autoscaling_config: horizon_client_sdk::types::UpdateContainerRequestAutoscaling =
                autoscaling_builder.try_into().map_err(|e| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Invalid autoscaling config: {:?}", e),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            request_builder = request_builder.autoscaling(autoscaling_config);
        }

        // Wire SA identity on updates
        {
            let sa_resource_id = format!("{}-sa", config.get_permissions());
            if let Some(sa_resource) = ctx.desired_stack.resources.get(&sa_resource_id) {
                let sa_ctrl = ctx
                    .require_dependency::<crate::service_account::AzureServiceAccountController>(
                        &(&sa_resource.config).into(),
                    )?;
                if let Some(client_id) = &sa_ctrl.identity_client_id {
                    let sa = horizon_client_sdk::types::NullableServiceAccountTarget::from(
                        horizon_client_sdk::types::AzureServiceAccountTarget {
                            client_id: client_id.parse().into_alien_error().context(
                                ErrorData::ResourceConfigInvalid {
                                    message: format!(
                                        "Invalid managed identity client ID '{}'",
                                        client_id
                                    ),
                                    resource_id: Some(config.id.clone()),
                                },
                            )?,
                            type_: horizon_client_sdk::types::AzureServiceAccountTargetType::Azure,
                        },
                    );
                    request_builder = request_builder.service_account(sa);
                }
            }
        }

        let request: horizon_client_sdk::types::UpdateContainerRequest =
            request_builder.try_into().map_err(|e| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Failed to build update request: {:?}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        horizon
            .client
            .update_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .body(&request)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to update container in Horizon: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        info!(container_id = %config.id, "Starting container deletion");

        Ok(HandlerAction::Continue {
            state: DeletingHorizonContainer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingHorizonContainer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_horizon_container(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;

        let cluster_id = config.cluster.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container must specify a cluster".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon = Self::horizon(ctx, cluster_id)?;

        info!(
            container_id = %config.id,
            cluster_id = %horizon.cluster.cluster_id,
            "Deleting container from Horizon"
        );

        let delete_result = horizon
            .client
            .delete_container()
            .cluster_id(&horizon.cluster.cluster_id)
            .name(&config.id)
            .send()
            .await;

        match delete_result {
            Ok(_) => info!(container_id = %config.id, "Container deleted from Horizon"),
            Err(e) => {
                warn!(container_id = %config.id, "Failed to delete container from Horizon: {}", e)
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingLoadBalancer,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingLoadBalancer,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_load_balancer(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let lb_client = ctx
            .service_provider
            .get_azure_load_balancer_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        if let Some(load_balancer_name) = &self.load_balancer_name {
            info!(load_balancer_name = %load_balancer_name, "Deleting load balancer");

            match lb_client
                .delete_load_balancer(&resource_group_name, load_balancer_name)
                .await
            {
                Ok(op_result) => {
                    op_result
                        .wait_for_operation_completion(
                            &*lro_client,
                            "DeleteLoadBalancer",
                            load_balancer_name,
                        )
                        .await
                        .ok();
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(load_balancer_name = %load_balancer_name, "Load balancer already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete load balancer {}", load_balancer_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.load_balancer_name = None;
        self.backend_pool_name = None;
        self.frontend_ip_config_name = None;
        self.probe_name = None;
        self.lb_rule_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingPublicIp,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingPublicIp,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_public_ip(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let network_client = ctx.service_provider.get_azure_network_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        if let Some(public_ip_name) = &self.public_ip_name {
            info!(public_ip_name = %public_ip_name, "Deleting public IP address");

            match network_client
                .delete_public_ip_address(&resource_group_name, public_ip_name)
                .await
            {
                Ok(op_result) => {
                    op_result
                        .wait_for_operation_completion(
                            &*lro_client,
                            "DeletePublicIpAddress",
                            public_ip_name,
                        )
                        .await
                        .ok();
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(public_ip_name = %public_ip_name, "Public IP already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete public IP {}", public_ip_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.public_ip_name = None;
        self.public_ip_id = None;
        self.public_url = None;

        Ok(HandlerAction::Continue {
            state: DeletingManagedDisks,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagedDisks,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_managed_disks(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let disk_client = ctx
            .service_provider
            .get_azure_managed_disks_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        for disk in &self.managed_disks {
            info!(disk_name = %disk.disk_name, "Deleting managed disk");

            match disk_client
                .delete_disk(&resource_group_name, &disk.disk_name)
                .await
            {
                Ok(op_result) => {
                    op_result
                        .wait_for_operation_completion(
                            &*lro_client,
                            "DeleteManagedDisk",
                            &disk.disk_name,
                        )
                        .await
                        .ok();
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(disk_name = %disk.disk_name, "Managed disk already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete disk {}", disk.disk_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.managed_disks.clear();

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    // ─────────────── TERMINAL STATES ─────────────────────────────

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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let container_name = self.container_name.as_ref()?;
        let status = self.horizon_status.unwrap_or(ContainerStatus::Pending);

        // Build load balancer endpoint for DNS controller
        let load_balancer_endpoint = self
            .load_balancer_name
            .as_ref()
            .and_then(|_| self.public_url.as_ref())
            .and_then(|url| {
                // Extract IP or hostname from URL
                url.strip_prefix("http://")
                    .or_else(|| url.strip_prefix("https://"))
                    .map(|s| s.split('/').next().unwrap_or(s))
                    .map(|dns| alien_core::LoadBalancerEndpoint {
                        dns_name: dns.to_string(),
                        hosted_zone_id: None, // Azure doesn't use hosted zones
                    })
            });

        Some(ResourceOutputs::new(ContainerOutputs {
            name: container_name.clone(),
            status,
            current_replicas: self.current_replicas,
            desired_replicas: self.current_replicas,
            internal_dns: format!("{}.svc", container_name),
            url: self.public_url.clone(),
            replicas: vec![],
            load_balancer_endpoint,
        }))
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        use alien_core::bindings::{BindingValue, ContainerBinding};

        self.container_name.as_ref().map(|name| {
            let internal_url = format!("http://{}.svc:8080", name);

            let binding = if let Some(url) = &self.public_url {
                ContainerBinding::horizon_with_public_url(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                    BindingValue::value(url.clone()),
                )
            } else {
                ContainerBinding::horizon(
                    BindingValue::value(name.clone()),
                    BindingValue::value(internal_url),
                )
            };

            serde_json::to_value(binding).unwrap_or_default()
        })
    }
}

impl AzureContainerController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(container_name: &str, replicas: u32, public_url: Option<String>) -> Self {
        Self {
            state: AzureContainerState::Ready,
            container_name: Some(container_name.to_string()),
            horizon_status: Some(ContainerStatus::Running),
            current_replicas: replicas,
            public_url,
            public_ip_name: Some("test-public-ip".to_string()),
            public_ip_id: Some("/subscriptions/test/resourceGroups/test-rg/providers/Microsoft.Network/publicIPAddresses/test".to_string()),
            load_balancer_name: Some("test-lb".to_string()),
            backend_pool_name: Some("test-pool".to_string()),
            frontend_ip_config_name: Some("test-frontend".to_string()),
            probe_name: Some("test-probe".to_string()),
            lb_rule_name: Some("test-rule".to_string()),
            fqdn: None,
            certificate_id: None,
            keyvault_cert_id: None,
            uses_custom_domain: false,
            certificate_issued_at: None,
            managed_disks: Vec::new(),
            wait_for_replicas_iterations: 0,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_azure_clients::azure::disks::MockManagedDisksApi;
    use alien_azure_clients::azure::keyvault::MockKeyVaultCertificatesApi;
    use alien_azure_clients::azure::load_balancers::MockLoadBalancerApi;
    use alien_azure_clients::azure::long_running_operation::{
        MockLongRunningOperationApi, OperationResult,
    };
    use alien_azure_clients::azure::models::certificates::CertificateBundle;
    use alien_azure_clients::azure::models::disk_rp::Disk;
    use alien_azure_clients::azure::models::public_ip_address::{
        PublicIpAddress, PublicIpAddressPropertiesFormat,
    };
    use alien_azure_clients::azure::network::MockNetworkApi;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::NetworkSettings;
    use alien_core::{
        CapacityGroup, CertificateStatus, ComputeBackend, ContainerAutoscaling, DnsRecordStatus,
        DomainMetadata, EnvironmentVariablesSnapshot, HorizonClusterConfig, HorizonConfig, Network,
        ResourceDomainInfo, ResourceSpec,
    };
    use httpmock::MockServer;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn create_test_domain_metadata(resource_id: &str) -> DomainMetadata {
        let mut resources = HashMap::new();
        resources.insert(
            resource_id.to_string(),
            ResourceDomainInfo {
                fqdn: format!("{}.test.example.com", resource_id),
                certificate_id: "test-cert-id".to_string(),
                certificate_status: CertificateStatus::Issued,
                dns_status: DnsRecordStatus::Active,
                dns_error: None,
                certificate_chain: Some(
                    "-----BEGIN CERTIFICATE-----\nMIIBtest\n-----END CERTIFICATE-----\n"
                        .to_string(),
                ),
                private_key: Some(
                    "-----BEGIN PRIVATE KEY-----\nMIIBtest\n-----END PRIVATE KEY-----\n"
                        .to_string(),
                ),
                issued_at: Some("2024-01-01T00:00:00Z".to_string()),
            },
        );
        DomainMetadata {
            base_domain: "test.example.com".to_string(),
            public_subdomain: "test".to_string(),
            hosted_zone_id: "Z1234567890ABC".to_string(),
            resources,
        }
    }

    fn setup_horizon_server(
        cluster_id: &str,
        container_name: &str,
        healthy_replicas: u32,
    ) -> MockServer {
        let server = MockServer::start();

        let replica_infos: Vec<serde_json::Value> = (0..healthy_replicas)
            .map(|idx| {
                json!({
                    "replicaId": format!("{}-{}", container_name, idx),
                    "machineId": format!("machine-{}", idx),
                    "ip": format!("10.0.1.{}", idx + 10),
                    "status": "running",
                    "healthy": true,
                    "consecutiveFailures": 0
                })
            })
            .collect();

        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": {
                "cpu": { "min": "1", "desired": "1" },
                "memory": { "min": "1Gi", "desired": "1Gi" }
            },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": replica_infos,
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path(format!("/clusters/{}/containers", cluster_id));
            then.status(200).json_body(create_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(create_response.clone());
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::DELETE).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(json!({ "success": true }));
        });

        server
    }

    fn setup_mock_provider(
        network: Arc<MockNetworkApi>,
        load_balancer: Arc<MockLoadBalancerApi>,
        disks: Arc<MockManagedDisksApi>,
        lro: Arc<MockLongRunningOperationApi>,
        key_vault: Option<Arc<MockKeyVaultCertificatesApi>>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_network_client()
            .returning(move |_| Ok(network.clone()));
        provider
            .expect_get_azure_load_balancer_client()
            .returning(move |_| Ok(load_balancer.clone()));
        provider
            .expect_get_azure_managed_disks_client()
            .returning(move |_| Ok(disks.clone()));
        provider
            .expect_get_azure_long_running_operation_client()
            .returning(move |_| Ok(lro.clone()));
        if let Some(kv) = key_vault {
            provider
                .expect_get_azure_key_vault_certificates_client()
                .returning(move |_| Ok(kv.clone()));
        }
        Arc::new(provider)
    }

    fn test_disk(id: Option<String>) -> Disk {
        Disk {
            location: "eastus".to_string(),
            sku: None,
            properties: Some(DiskProperties {
                creation_data: CreationData {
                    create_option: DiskCreateOption::Empty,
                    elastic_san_resource_id: None,
                    gallery_image_reference: None,
                    image_reference: None,
                    instant_access_duration_minutes: None,
                    logical_sector_size: None,
                    performance_plus: None,
                    provisioned_bandwidth_copy_speed: None,
                    security_data_uri: None,
                    security_metadata_uri: None,
                    source_resource_id: None,
                    source_unique_id: None,
                    source_uri: None,
                    storage_account_id: None,
                    upload_size_bytes: None,
                },
                disk_size_gb: Some(10),
                disk_iops_read_write: None,
                disk_m_bps_read_write: None,
                disk_iops_read_only: None,
                disk_m_bps_read_only: None,
                availability_policy: None,
                bursting_enabled: None,
                bursting_enabled_time: None,
                completion_percent: None,
                data_access_auth_mode: None,
                disk_access_id: None,
                disk_size_bytes: None,
                disk_state: None,
                encryption: None,
                encryption_settings_collection: None,
                hyper_v_generation: None,
                last_ownership_update_time: None,
                max_shares: None,
                network_access_policy: None,
                optimized_for_frequent_attach: None,
                os_type: None,
                property_updates_in_progress: None,
                provisioning_state: None,
                public_network_access: None,
                purchase_plan: None,
                security_profile: None,
                share_info: vec![],
                supported_capabilities: None,
                supports_hibernation: None,
                tier: None,
                time_created: None,
                unique_id: None,
            }),
            tags: HashMap::new(),
            id,
            name: None,
            type_: None,
            managed_by: None,
            managed_by_extended: vec![],
            extended_location: None,
            zones: vec![],
            system_data: None,
        }
    }

    fn mock_clients_for_create_delete(
        ip: &str,
    ) -> (
        Arc<MockNetworkApi>,
        Arc<MockLoadBalancerApi>,
        Arc<MockManagedDisksApi>,
        Arc<MockLongRunningOperationApi>,
        Arc<MockKeyVaultCertificatesApi>,
    ) {
        let mut network = MockNetworkApi::new();
        let mut load_balancer = MockLoadBalancerApi::new();
        let mut disks = MockManagedDisksApi::new();
        let lro = Arc::new(MockLongRunningOperationApi::new());
        let mut key_vault = MockKeyVaultCertificatesApi::new();

        key_vault
            .expect_import_certificate()
            .returning(|_, _, _| Ok(CertificateBundle::default()));

        network
            .expect_create_or_update_public_ip_address()
            .returning(|_, _, _| Ok(OperationResult::Completed(PublicIpAddress::default())));
        let ip = ip.to_string();
        network
            .expect_get_public_ip_address()
            .returning(move |_, _| {
                Ok(PublicIpAddress {
                    id: Some(format!("/subscriptions/test/resourceGroups/test/providers/Microsoft.Network/publicIPAddresses/{}", ip)),
                    properties: Some(Box::new(PublicIpAddressPropertiesFormat {
                        ip_address: Some(ip.clone()),
                        ..Default::default()
                    })),
                    ..Default::default()
                })
            });
        network
            .expect_delete_public_ip_address()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        load_balancer
            .expect_create_or_update_load_balancer()
            .returning(|_, _, _| Ok(OperationResult::Completed(LoadBalancer::default())));
        load_balancer
            .expect_delete_load_balancer()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        disks
            .expect_create_or_update_disk()
            .returning(|_, _, _| Ok(OperationResult::Completed(test_disk(None))));
        disks
            .expect_get_disk()
            .returning(|_, _| {
                Ok(test_disk(Some(
                    "/subscriptions/test/resourceGroups/test/providers/Microsoft.Compute/disks/test-disk".to_string(),
                )))
            });
        disks
            .expect_delete_disk()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        (
            Arc::new(network),
            Arc::new(load_balancer),
            Arc::new(disks),
            lro,
            Arc::new(key_vault),
        )
    }

    fn mock_clients_for_best_effort_delete() -> (
        Arc<MockNetworkApi>,
        Arc<MockLoadBalancerApi>,
        Arc<MockManagedDisksApi>,
        Arc<MockLongRunningOperationApi>,
    ) {
        let mut network = MockNetworkApi::new();
        let mut load_balancer = MockLoadBalancerApi::new();
        let mut disks = MockManagedDisksApi::new();
        let lro = Arc::new(MockLongRunningOperationApi::new());

        let not_found = || {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Azure".to_string(),
                    resource_name: "missing".to_string(),
                },
            ))
        };

        network
            .expect_delete_public_ip_address()
            .returning(move |_, _| not_found());
        load_balancer
            .expect_delete_load_balancer()
            .returning(move |_, _| not_found());
        disks
            .expect_delete_disk()
            .returning(move |_, _| not_found());

        (
            Arc::new(network),
            Arc::new(load_balancer),
            Arc::new(disks),
            lro,
        )
    }

    fn test_horizon_config(server: &MockServer, cluster_id: &str) -> ComputeBackend {
        let mut clusters = HashMap::new();
        clusters.insert(
            "compute".to_string(),
            HorizonClusterConfig {
                cluster_id: cluster_id.to_string(),
                management_token: "hm_test".to_string(),
            },
        );

        ComputeBackend::Horizon(HorizonConfig {
            url: server.base_url(),
            horizond_download_base_url: "http://releases.test".to_string(),
            horizond_binary_hash: None,
            clusters,
        })
    }

    fn test_container(cluster_id: &str) -> Container {
        Container::new("api".to_string())
            .cluster(cluster_id.to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .stateful(true)
            .replicas(1)
            .persistent_storage(alien_core::PersistentStorage {
                size: "10Gi".to_string(),
                mount_path: "/data".to_string(),
                storage_type: None,
                iops: None,
                throughput: None,
            })
            .permissions("execution".to_string())
            .build()
    }

    fn test_network() -> Network {
        Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: Some("10.0.0.0/16".to_string()),
                availability_zones: 2,
            })
            .build()
    }

    fn test_cluster_resource() -> ContainerCluster {
        ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("Standard_B2s".to_string()),
                profile: None,
                min_size: 1,
                max_size: 1,
            })
            .build()
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let (network, load_balancer, disks, lro, key_vault) =
            mock_clients_for_create_delete("203.0.113.10");
        let mock_provider =
            setup_mock_provider(network, load_balancer, disks, lro, Some(key_vault));

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(AzureContainerController::default())
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .domain_metadata(create_test_domain_metadata(container_name))
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_update_flow_succeeds() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let (network, load_balancer, disks, lro, key_vault) =
            mock_clients_for_create_delete("203.0.113.11");
        let mock_provider =
            setup_mock_provider(network, load_balancer, disks, lro, Some(key_vault));

        let mut container = test_container("compute");
        container.code = ContainerCode::Image {
            image: "nginx:1.25".to_string(),
        };

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 2,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: None,
                max_http_p95_latency_ms: None,
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = AzureContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.11".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(container)
            .controller(ready_controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing() {
        let cluster_id = "test-cluster";
        let container_name = "api";
        let server = setup_horizon_server(cluster_id, container_name, 1);
        let (network, load_balancer, disks, lro) = mock_clients_for_best_effort_delete();
        let mock_provider = setup_mock_provider(network, load_balancer, disks, lro, None);

        let mut controller = AzureContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.12".to_string()),
        );
        controller.managed_disks.push(ManagedDiskState {
            disk_id:
                "/subscriptions/test/resourceGroups/test/providers/Microsoft.Compute/disks/missing"
                    .to_string(),
            disk_name: "missing-disk".to_string(),
            zone: "1".to_string(),
            ordinal: 0,
            size_gb: 10,
        });

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    /// Verifies that updates with target_http_in_flight_per_replica and max_http_p95_latency_ms complete.
    #[tokio::test]
    async fn test_update_with_http_in_flight() {
        let cluster_id = "test-cluster";
        let container_name = "api";

        let server = MockServer::start();
        let create_response = json!({
            "name": container_name,
            "clusterId": cluster_id,
            "status": "running",
            "createdAt": "2024-01-01T00:00:00Z"
        });
        let container_response = json!({
            "name": container_name,
            "capacityGroup": "general",
            "image": "nginx:latest",
            "resources": { "cpu": { "min": "1", "desired": "1" }, "memory": { "min": "1Gi", "desired": "1Gi" } },
            "stateful": false,
            "ports": [8080],
            "status": "running",
            "clusterId": cluster_id,
            "replicasInfo": [{ "replicaId": "api-0", "machineId": "m-0", "ip": "10.0.1.10", "status": "running", "healthy": true, "consecutiveFailures": 0 }],
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z"
        });

        server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!(
                    "/clusters/{}/containers/{}",
                    cluster_id, container_name
                ))
                .body_contains("targetHttpInFlightPerReplica")
                .body_contains("maxHttpP95LatencyMs");
            then.status(200).json_body(create_response.clone());
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET).path(format!(
                "/clusters/{}/containers/{}",
                cluster_id, container_name
            ));
            then.status(200).json_body(container_response.clone());
        });

        let (network, load_balancer, disks, lro, key_vault) =
            mock_clients_for_create_delete("203.0.113.12");
        let mock_provider =
            setup_mock_provider(network, load_balancer, disks, lro, Some(key_vault));

        let updated_container = Container::new("api".to_string())
            .cluster("compute".to_string())
            .code(ContainerCode::Image {
                image: "nginx:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "1".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "1Gi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .expose_port(8080, alien_core::ExposeProtocol::Http)
            .autoscaling(ContainerAutoscaling {
                min: 1,
                desired: 1,
                max: 5,
                target_cpu_percent: Some(70.0),
                target_memory_percent: Some(80.0),
                target_http_in_flight_per_replica: Some(10),
                max_http_p95_latency_ms: Some(200.0),
            })
            .permissions("execution".to_string())
            .build();

        let ready_controller = AzureContainerController::mock_ready(
            container_name,
            1,
            Some("http://203.0.113.12".to_string()),
        );

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_container("compute"))
            .controller(ready_controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(&server, cluster_id))
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_dependency(
                test_cluster_resource(),
                AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.update(updated_container).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
