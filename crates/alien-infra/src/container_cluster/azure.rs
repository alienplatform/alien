//! Azure ContainerCluster Controller
//!
//! This module implements the Azure-specific controller for managing ContainerCluster resources.
//! A ContainerCluster provisions the compute infrastructure for running containers via Horizon:
//!
//! - Managed Identity for VM instances (to access Key Vault, call Azure APIs, etc.)
//! - Key Vault secret for horizond machine token
//! - Network Security Group for instance communication
//! - Virtual Machine Scale Sets (one per capacity group)
//!
//! The platform creates the Horizon cluster via the Horizon API before deployment.
//! This controller provisions the cloud infrastructure that machines use to join the cluster.

use alien_azure_clients::azure::compute::{RollingUpgradeLatestStatus, VirtualMachineScaleSetsApi};
use alien_azure_clients::azure::models::authorization_role_assignments::{
    RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
};
use alien_azure_clients::azure::models::compute_rp::{
    ApiEntityReference, BootDiagnostics, CachingTypes, DiagnosticsProfile, DiskCreateOptionTypes,
    ImageReference, LinuxConfiguration, ResourceIdentityType, Sku, SshConfiguration,
    StorageAccountTypes, SubResource, UserAssignedIdentitiesValue, VirtualMachineScaleSet,
    VirtualMachineScaleSetExtension, VirtualMachineScaleSetExtensionProfile,
    VirtualMachineScaleSetExtensionProperties, VirtualMachineScaleSetIdentity,
    VirtualMachineScaleSetIpConfiguration, VirtualMachineScaleSetIpConfigurationProperties,
    VirtualMachineScaleSetManagedDiskParameters, VirtualMachineScaleSetNetworkConfiguration,
    VirtualMachineScaleSetNetworkConfigurationProperties, VirtualMachineScaleSetNetworkProfile,
    VirtualMachineScaleSetOsDisk, VirtualMachineScaleSetOsProfile,
    VirtualMachineScaleSetProperties, VirtualMachineScaleSetStorageProfile,
    VirtualMachineScaleSetVmProfile,
};
use alien_azure_clients::azure::models::managed_identity::Identity;
use alien_azure_clients::azure::models::network_security_group::{
    NetworkSecurityGroup, NetworkSecurityGroupPropertiesFormat, SecurityRule, SecurityRuleAccess,
    SecurityRuleDirection, SecurityRulePropertiesFormat, SecurityRulePropertiesFormatProtocol,
};
use alien_azure_clients::azure::models::secrets::SecretSetParameters;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CapacityGroup, CapacityGroupStatus, ComputeBackend, ContainerCluster, ContainerClusterOutputs,
    Network, ResourceOutputs, ResourceRef, ResourceStatus, TemplateInputs,
};
use alien_error::{AlienError, Context};
use alien_macros::controller;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, to_horizon_capacity_groups};
use crate::infra_requirements::azure_utils;
use crate::network::AzureNetworkController;

/// Tracks the state of a single Virtual Machine Scale Set (one per capacity group).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VmssState {
    /// Capacity group ID this VMSS is for
    pub group_id: String,
    /// VMSS name
    pub vmss_name: Option<String>,
    /// Resource group name
    pub resource_group: Option<String>,
    /// Current number of instances
    pub current_size: u32,
    /// Desired number of instances (from capacity plan)
    pub desired_size: u32,
    /// Instance type (VM SKU) used
    pub instance_type: Option<String>,
}

/// Azure ContainerCluster Controller state machine.
///
/// This controller manages the lifecycle of Azure infrastructure for container workloads:
/// - Managed Identity for VM instances
/// - Key Vault secret for machine token storage
/// - Network Security Group for cluster networking
/// - Virtual Machine Scale Sets (one per capacity group)
#[controller]
pub struct AzureContainerClusterController {
    // Managed Identity
    pub(crate) identity_id: Option<String>,
    pub(crate) identity_principal_id: Option<String>,
    pub(crate) identity_client_id: Option<String>,

    // Key Vault (for machine token and OTLP auth headers)
    pub(crate) key_vault_name: Option<String>,
    pub(crate) machine_token_secret_name: Option<String>,
    /// Key Vault secret name for the OTLP logs auth header (optional).
    #[serde(default)]
    pub(crate) otlp_auth_secret_name: Option<String>,
    /// Key Vault secret name for the OTLP metrics auth header (optional).
    /// Only set when metrics uses a separate auth header from logs (e.g. different Axiom dataset).
    #[serde(default)]
    pub(crate) otlp_metrics_auth_secret_name: Option<String>,

    // Network Security Group
    pub(crate) nsg_id: Option<String>,
    pub(crate) nsg_name: Option<String>,

    // Virtual Machine Scale Sets (one per capacity group)
    pub(crate) vmss_states: HashMap<String, VmssState>,

    // Role assignments for managed identity
    #[serde(default)]
    pub(crate) role_assignment_ids: Vec<String>,

    // Horizon cluster info
    pub(crate) horizon_cluster_id: Option<String>,
    pub(crate) horizon_ready: bool,
    pub(crate) horizon_api_url: Option<String>,

    // Boot diagnostics: counts iterations spent waiting for VMSSes to become ready.
    #[serde(default)]
    pub(crate) boot_check_iterations: u32,

    /// Groups newly created during an update flow, waiting for their instances to provision.
    #[serde(default)]
    pub(crate) new_groups_pending_ready: Vec<String>,

    /// Whether a rolling update was triggered in the current update cycle.
    #[serde(default)]
    pub(crate) rolling_update_triggered: bool,

    /// Counts iterations spent waiting for rolling updates to complete.
    #[serde(default)]
    pub(crate) rolling_update_poll_iterations: u32,
}

// BOOT_DIAG_TIMEOUT_ITERATIONS: number of 30-second polling iterations (~15 minutes).
const BOOT_DIAG_TIMEOUT_ITERATIONS: u32 = 30;

impl AzureContainerClusterController {
    fn machine_token_key(resource_id: &str) -> String {
        format!(
            "ALIEN_HORIZON_MACHINE_TOKEN_{}",
            resource_id.to_uppercase().replace('-', "_")
        )
    }

    fn machine_token_from_env(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<String> {
        let token_key = Self::machine_token_key(resource_id);
        let token = ctx
            .deployment_config
            .environment_variables
            .variables
            .iter()
            .find(|var| var.name == token_key)
            .map(|var| var.value.clone())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("Missing machine token environment variable '{}'", token_key),
                    resource_id: Some(resource_id.to_string()),
                })
            })?;

        Ok(token)
    }

    fn role_assignment_uuid(seed: &str) -> String {
        Uuid::new_v5(&Uuid::NAMESPACE_OID, seed.as_bytes()).to_string()
    }

    fn contributor_role_definition_id(subscription_id: &str) -> String {
        format!(
            "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            subscription_id, "b24988ac-6180-42a0-ab88-20f7382dd24c"
        )
    }

    fn key_vault_secrets_user_role_definition_id(subscription_id: &str) -> String {
        format!(
            "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{}",
            subscription_id, "4633458b-17de-408a-b874-0445c86b69e6"
        )
    }

    fn health_extension_profile() -> VirtualMachineScaleSetExtensionProfile {
        VirtualMachineScaleSetExtensionProfile {
            extensions: vec![VirtualMachineScaleSetExtension {
                name: Some("HealthExtension".to_string()),
                properties: Some(VirtualMachineScaleSetExtensionProperties {
                    publisher: Some("Microsoft.ManagedServices".to_string()),
                    type_: Some("ApplicationHealthLinux".to_string()),
                    type_handler_version: Some("1.0".to_string()),
                    auto_upgrade_minor_version: Some(true),
                    settings: Some(serde_json::json!({
                        "protocol": "http",
                        "port": 8080,
                        "requestPath": "/health"
                    })),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            extensions_time_budget: None,
        }
    }

    /// Generate cloud-init script for horizond.
    fn generate_cloud_init(
        &self,
        cluster_id: &str,
        api_url: &str,
        horizond_download_base_url: &str,
        key_vault_name: &str,
        secret_name: &str,
        capacity_group_prefix: &str,
        otlp_logs_endpoint: Option<&str>,
        otlp_metrics_endpoint: Option<&str>,
        otlp_auth_secret_name: Option<&str>,
        otlp_metrics_auth_secret_name: Option<&str>,
    ) -> String {
        // Cloud-init script that:
        // 1. Fetches machine token (and optionally OTLP auth headers) from Key Vault
        // 2. Installs horizond dependencies
        // 3. Starts horizond with cluster configuration
        let horizond_url =
            super::join_url_path(horizond_download_base_url, "linux-x86_64/horizond");

        // Build the optional block that fetches the OTLP logs auth header from Key Vault.
        let otlp_secret_fetch = match otlp_auth_secret_name {
            Some(sn) => format!(
                r#"
      # Fetch OTLP logs auth header from Key Vault
      OTLP_AUTH_HEADER=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" "https://{{}}.vault.azure.net/secrets/{}/versions/latest?api-version=7.4" | jq -r .value)"#,
                sn
            ),
            None => String::new(),
        };

        // Fetch the OTLP metrics auth header from a separate secret if configured.
        let otlp_metrics_secret_fetch = match otlp_metrics_auth_secret_name {
            Some(sn) => format!(
                r#"
      # Fetch OTLP metrics auth header from Key Vault
      OTLP_METRICS_AUTH_HEADER=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" "https://{{}}.vault.azure.net/secrets/{}/versions/latest?api-version=7.4" | jq -r .value)"#,
                sn
            ),
            None => String::new(),
        };

        // Build optional OTLP flags for the horizond ExecStart.
        let otlp_flags = {
            let mut flags = Vec::new();
            let has_logs_auth = otlp_auth_secret_name.is_some();
            let metrics_auth_var = if otlp_metrics_auth_secret_name.is_some() {
                "$OTLP_METRICS_AUTH_HEADER"
            } else {
                "$OTLP_AUTH_HEADER"
            };
            let indent = "        ";

            if let Some(ep) = otlp_logs_endpoint {
                flags.push(format!("{}--otlp-logs-endpoint \"{}\" \\", indent, ep));
                if has_logs_auth {
                    flags.push(format!(
                        "{}--otlp-logs-auth-header \"$OTLP_AUTH_HEADER\" \\",
                        indent
                    ));
                }
            }
            if let Some(ep) = otlp_metrics_endpoint {
                flags.push(format!("{}--otlp-metrics-endpoint \"{}\" \\", indent, ep));
                if has_logs_auth {
                    flags.push(format!(
                        "{}--otlp-metrics-auth-header \"{}\" \\",
                        indent, metrics_auth_var
                    ));
                }
            }
            flags.join("\n")
        };

        format!(
            r#"#cloud-config
write_files:
  - path: /etc/horizond/fetch-token.sh
    permissions: '0755'
    content: |
      #!/bin/bash
      set -euo pipefail

      log() {{ echo "[HORIZON-BOOT] $1"; }}
      trap 'log "error: script failed at line $LINENO (exit $?)"' ERR

      log "packages_installing"
      apt-get update -qq
      DEBIAN_FRONTEND=noninteractive apt-get install -y -qq containerd wireguard-tools curl jq tar
      systemctl enable --now containerd || true
      log "packages_installed"

      log "dependencies_installing"
      mkdir -p /opt/cni/bin /etc/cni/net.d /etc/wireguard /etc/horizond /var/lib/horizond
      ARCH=$(dpkg --print-architecture)
      curl -fsSL "https://github.com/containernetworking/plugins/releases/download/v1.4.0/cni-plugins-linux-${{ARCH}}-v1.4.0.tgz" | tar -C /opt/cni/bin -xz
      curl -fsSL "https://github.com/containerd/nerdctl/releases/download/v1.7.2/nerdctl-1.7.2-linux-${{ARCH}}.tar.gz" | tar -C /usr/local/bin -xz
      log "dependencies_installed"

      # Get access token for Key Vault using managed identity
      ACCESS_TOKEN=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https%3A%2F%2Fvault.azure.net" | jq -r .access_token)

      # Fetch machine token from Key Vault
      MACHINE_TOKEN=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" "https://{}.vault.azure.net/secrets/{}/versions/latest?api-version=7.4" | jq -r .value)
{}

      VM_NAME=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/instance/compute/name?api-version=2021-02-01&format=text")
      ZONE=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/instance/compute/zone?api-version=2021-02-01&format=text")
      VMSS_NAME=$(curl -s -H Metadata:true "http://169.254.169.254/metadata/instance/compute/vmScaleSetName?api-version=2021-02-01&format=text")
      CAPACITY_GROUP="${{VMSS_NAME#{}}}"
      if [ -z "$CAPACITY_GROUP" ] || [ "$CAPACITY_GROUP" = "$VMSS_NAME" ]; then
        CAPACITY_GROUP="general"
      fi

      log "horizond_downloading"
      curl -fsSL {} -o /usr/local/bin/horizond
      chmod +x /usr/local/bin/horizond
      log "horizond_downloaded"

      cat > /etc/systemd/system/horizond.service <<EOF
      [Unit]
      Description=Horizon Machine Agent
      After=network-online.target containerd.service
      Wants=network-online.target
      Requires=containerd.service

      [Service]
      Type=simple
      User=root
      ExecStart=/usr/local/bin/horizond \\
        --cluster-id "{}" \\
        --machine-id $VM_NAME \\
        --machine-token $MACHINE_TOKEN \\
        --api-url "{}" \\
        --zone $ZONE \\
        --network-interface eth0 \\
        --capacity-group $CAPACITY_GROUP{}
      Restart=on-failure
      RestartSec=5
      StandardOutput=journal
      StandardError=journal
      LimitNOFILE=1048576
      LimitNPROC=infinity
      LimitCORE=infinity
      TasksMax=infinity

      [Install]
      WantedBy=multi-user.target
      EOF

runcmd:
  - mkdir -p /etc/horizond
  - /etc/horizond/fetch-token.sh
  - systemctl daemon-reload
  - systemctl enable horizond
  - echo "[HORIZON-BOOT] horizond_starting"
  - systemctl start horizond
"#,
            key_vault_name,
            secret_name,
            format!("{}{}", otlp_secret_fetch, otlp_metrics_secret_fetch),
            capacity_group_prefix,
            horizond_url,
            cluster_id,
            api_url,
            if otlp_flags.is_empty() {
                String::new()
            } else {
                let trimmed = otlp_flags.trim_end_matches(" \\").trim_end_matches('\\');
                format!(" \\\n{}", trimmed)
            }
        )
    }

    /// Deletes a VMSS for a capacity group (best-effort).
    async fn delete_capacity_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group_id: &str,
        resource_id: &str,
    ) -> Result<()> {
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
        if let Some(state) = self.vmss_states.remove(group_id) {
            if let (Some(vmss_name), Some(rg)) = (state.vmss_name, state.resource_group) {
                let _ = compute_client.delete_vmss(&rg, &vmss_name).await;
                info!(vmss_name = %vmss_name, "Deleted VMSS for removed capacity group");
            }
        }
        let _ = resource_id;
        Ok(())
    }

    /// Tries to fetch the serial console log from one VMSS instance for boot diagnostics.
    async fn collect_serial_console_log(
        compute_client: &dyn VirtualMachineScaleSetsApi,
        vmss_states: &HashMap<String, VmssState>,
    ) -> Option<String> {
        for state in vmss_states.values() {
            if let (Some(vmss_name), Some(resource_group)) =
                (&state.vmss_name, &state.resource_group)
            {
                let vms = compute_client
                    .list_vmss_vms(resource_group, vmss_name)
                    .await
                    .ok()?;

                let instance_id = vms.value.into_iter().next().and_then(|vm| vm.instance_id)?;

                let log = compute_client
                    .get_vmss_vm_serial_console_log(resource_group, vmss_name, &instance_id)
                    .await
                    .ok()?;

                if !log.is_empty() {
                    return Some(log);
                }
            }
        }
        None
    }
}

#[controller]
impl AzureContainerClusterController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(
            cluster_id = %config.id,
            "Starting Azure ContainerCluster provisioning"
        );

        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "ContainerCluster resources require a Horizon compute backend"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };

        let cluster_config = horizon_config.clusters.get(&config.id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("No Horizon cluster config for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let template_inputs = config.template_inputs.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "ContainerCluster is missing template_inputs (stamp_template_inputs did not run)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        self.horizon_cluster_id = Some(cluster_config.cluster_id.clone());
        self.horizon_api_url = Some(template_inputs.horizon_api_url.clone());
        self.horizon_ready = false;

        Ok(HandlerAction::Continue {
            state: CreatingManagedIdentity,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingManagedIdentity,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let identity_client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let identity_name = format!("{}-{}-identity", ctx.resource_prefix, config.id);

        info!(
            identity_name = %identity_name,
            resource_group = %resource_group_name,
            "Creating user-assigned managed identity for container instances"
        );

        // Check if identity already exists (idempotency)
        let identity = match identity_client
            .get_user_assigned_identity(&resource_group_name, &identity_name)
            .await
        {
            Ok(existing) => {
                info!("Managed identity already exists, reusing");
                existing
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                let location = azure_cfg
                    .region
                    .clone()
                    .unwrap_or_else(|| "eastus".to_string());

                // Create new identity
                let new_identity = Identity {
                    id: None,
                    location,
                    name: None,
                    properties: None,
                    system_data: None,
                    tags: HashMap::new(),
                    type_: None,
                };

                identity_client
                    .create_or_update_user_assigned_identity(
                        &resource_group_name,
                        &identity_name,
                        &new_identity,
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create managed identity".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?
            }
            Err(e) => {
                return Err(e).context(ErrorData::CloudPlatformError {
                    message: "Failed to check managed identity existence".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            }
        };

        self.identity_id = identity.id.clone();
        self.identity_principal_id = identity
            .properties
            .as_ref()
            .and_then(|props| props.principal_id.as_ref())
            .map(|id| id.to_string());
        self.identity_client_id = identity
            .properties
            .as_ref()
            .and_then(|props| props.client_id.as_ref())
            .map(|id| id.to_string());

        info!(
            identity_id = ?identity.id,
            principal_id = ?self.identity_principal_id,
            client_id = ?self.identity_client_id,
            "Managed identity created/verified"
        );

        Ok(HandlerAction::Continue {
            state: AssigningIdentityRoles,
            suggested_delay: Some(Duration::from_secs(10)), // Wait for identity propagation
        })
    }

    #[handler(
        state = AssigningIdentityRoles,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn assigning_identity_roles(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let principal_id = self.identity_principal_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Identity principal ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            principal_id = %principal_id,
            "Assigning IAM roles to managed identity"
        );

        let contributor_role_definition_id =
            Self::contributor_role_definition_id(&azure_cfg.subscription_id);

        let role_assignment_name = Self::role_assignment_uuid(&format!(
            "{}-{}-contributor",
            ctx.resource_prefix, config.id
        ));
        let role_assignment_id = auth_client.build_resource_group_role_assignment_id(
            resource_group_name.clone(),
            role_assignment_name,
        );

        let role_assignment = RoleAssignment {
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.clone(),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                role_definition_id: contributor_role_definition_id,
                scope: None,
                condition: None,
                condition_version: None,
                created_by: None,
                created_on: None,
                delegated_managed_identity_resource_id: None,
                description: Some(
                    "Alien ContainerCluster managed identity contributor role".to_string(),
                ),
                updated_by: None,
                updated_on: None,
            }),
            ..Default::default()
        };

        auth_client
            .create_or_update_role_assignment_by_id(role_assignment_id.clone(), &role_assignment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to assign Contributor role to managed identity".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.role_assignment_ids.push(role_assignment_id);

        info!("Managed identity roles assigned");

        Ok(HandlerAction::Continue {
            state: CreatingHorizonCluster,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingHorizonCluster,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_horizon_cluster(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let cluster_id = self.horizon_cluster_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon cluster ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let api_url = self.horizon_api_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon API URL not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Update Horizon cluster with capacity groups (they were empty at creation time).
        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "ContainerCluster resources require a Horizon compute backend"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };

        let cluster_config = horizon_config.clusters.get(&config.id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("No Horizon cluster config for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let horizon_client = create_horizon_client(api_url, &cluster_config.management_token)
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Horizon client: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        let horizon_groups = to_horizon_capacity_groups(&config.capacity_groups, &config.id)?;
        let update_body = horizon_client_sdk::types::UpdateClusterRequest {
            capacity_groups: horizon_groups,
        };

        horizon_client
            .update_cluster()
            .cluster_id(cluster_id)
            .body(&update_body)
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to update Horizon cluster capacity groups: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        info!(
            cluster_id = %cluster_id,
            groups = config.capacity_groups.len(),
            "Updated Horizon cluster with capacity groups"
        );

        self.horizon_ready = true;

        Ok(HandlerAction::Continue {
            state: StoringMachineToken,
            suggested_delay: None,
        })
    }

    #[handler(
        state = StoringMachineToken,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn storing_machine_token(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let kv_client = ctx
            .service_provider
            .get_azure_key_vault_secrets_client(azure_cfg)?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        // For Azure Key Vault, we need a vault name
        let vault_name = format!("{}-secrets", ctx.resource_prefix);
        let secret_name = Self::machine_token_key(&config.id);
        let machine_token = Self::machine_token_from_env(ctx, &config.id)?;

        info!(
            vault_name = %vault_name,
            secret_name = %secret_name,
            "Storing machine token in Key Vault"
        );

        let vault_base_url = format!("https://{}.vault.azure.net", vault_name);

        // Create or update secret in Key Vault
        let secret = SecretSetParameters {
            attributes: None,
            content_type: Some("text/plain".to_string()),
            tags: HashMap::new(),
            value: machine_token,
        };

        kv_client
            .set_secret(vault_base_url.clone(), secret_name.clone(), secret)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to store secret in Key Vault".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let principal_id = self.identity_principal_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Identity principal ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let role_assignment_name = Self::role_assignment_uuid(&format!(
            "{}-{}-keyvault-secrets-user",
            ctx.resource_prefix, config.id
        ));

        let role_assignment_id = auth_client.build_resource_role_assignment_id(
            resource_group_name.clone(),
            "Microsoft.KeyVault".to_string(),
            None,
            "vaults".to_string(),
            vault_name.clone(),
            role_assignment_name,
        );

        let role_definition_id =
            Self::key_vault_secrets_user_role_definition_id(&azure_cfg.subscription_id);

        let role_assignment = RoleAssignment {
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.clone(),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                role_definition_id,
                scope: None,
                condition: None,
                condition_version: None,
                created_by: None,
                created_on: None,
                delegated_managed_identity_resource_id: None,
                description: Some(
                    "Alien ContainerCluster Key Vault secret read access".to_string(),
                ),
                updated_by: None,
                updated_on: None,
            }),
            ..Default::default()
        };

        auth_client
            .create_or_update_role_assignment_by_id(role_assignment_id.clone(), &role_assignment)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to assign Key Vault Secrets User role".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.key_vault_name = Some(vault_name.clone());
        self.machine_token_secret_name = Some(secret_name);
        self.role_assignment_ids.push(role_assignment_id);

        info!("Machine token stored in Key Vault");

        // If OTLP monitoring is configured, also store the auth headers as Key Vault secrets.
        // Same vault, same IAM grant — the managed identity can already read all secrets in the vault.
        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let vault_base_url = format!("https://{}.vault.azure.net", vault_name);

            // Store the logs auth header
            let logs_secret_name = "ALIEN-OTLP-AUTH-HEADER".to_string();
            info!(vault_name = %vault_name, secret_name = %logs_secret_name, "Storing OTLP logs auth header in Key Vault");
            kv_client
                .set_secret(
                    vault_base_url.clone(),
                    logs_secret_name.clone(),
                    SecretSetParameters {
                        attributes: None,
                        content_type: Some("text/plain".to_string()),
                        tags: HashMap::new(),
                        value: monitoring.logs_auth_header.clone(),
                    },
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to store OTLP logs auth header in Key Vault".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            self.otlp_auth_secret_name = Some(logs_secret_name);

            // Store the metrics auth header if it differs from logs
            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_name = "ALIEN-OTLP-METRICS-AUTH-HEADER".to_string();
                info!(vault_name = %vault_name, secret_name = %metrics_secret_name, "Storing OTLP metrics auth header in Key Vault");
                kv_client
                    .set_secret(
                        vault_base_url.clone(),
                        metrics_secret_name.clone(),
                        SecretSetParameters {
                            attributes: None,
                            content_type: Some("text/plain".to_string()),
                            tags: HashMap::new(),
                            value: metrics_auth_header.clone(),
                        },
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to store OTLP metrics auth header in Key Vault"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_name);
            }

            info!("OTLP auth headers stored in Key Vault");
        }

        Ok(HandlerAction::Continue {
            state: CreatingNetworkSecurityGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNetworkSecurityGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_network_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let network_client = ctx.service_provider.get_azure_network_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let nsg_name = format!("{}-{}-nsg", ctx.resource_prefix, config.id);

        info!(
            nsg_name = %nsg_name,
            resource_group = %resource_group_name,
            "Creating Network Security Group"
        );

        let location = azure_cfg
            .region
            .clone()
            .unwrap_or_else(|| "eastus".to_string());

        let make_inbound_rule = |name: &str,
                                 priority: i32,
                                 protocol: SecurityRulePropertiesFormatProtocol,
                                 dest_port: &str| {
            SecurityRule {
                name: Some(name.to_string()),
                properties: Some(SecurityRulePropertiesFormat {
                    access: SecurityRuleAccess::Allow,
                    description: None,
                    destination_address_prefix: Some("*".to_string()),
                    destination_address_prefixes: vec![],
                    destination_application_security_groups: vec![],
                    destination_port_range: Some(dest_port.to_string()),
                    destination_port_ranges: vec![],
                    direction: SecurityRuleDirection::Inbound,
                    priority,
                    protocol,
                    provisioning_state: None,
                    source_address_prefix: Some("*".to_string()),
                    source_address_prefixes: vec![],
                    source_application_security_groups: vec![],
                    source_port_range: Some("*".to_string()),
                    source_port_ranges: vec![],
                }),
                ..Default::default()
            }
        };

        let nsg = NetworkSecurityGroup {
            location: Some(location),
            properties: Some(NetworkSecurityGroupPropertiesFormat {
                security_rules: vec![
                    make_inbound_rule(
                        "AllowWireGuard",
                        100,
                        SecurityRulePropertiesFormatProtocol::Udp,
                        "51820",
                    ),
                    make_inbound_rule(
                        "AllowContainerPorts",
                        110,
                        SecurityRulePropertiesFormatProtocol::Tcp,
                        "8001-8999",
                    ),
                    make_inbound_rule(
                        "AllowHTTP",
                        120,
                        SecurityRulePropertiesFormatProtocol::Tcp,
                        "80",
                    ),
                    make_inbound_rule(
                        "AllowHTTPS",
                        130,
                        SecurityRulePropertiesFormatProtocol::Tcp,
                        "443",
                    ),
                    make_inbound_rule(
                        "AllowSSH",
                        140,
                        SecurityRulePropertiesFormatProtocol::Tcp,
                        "22",
                    ),
                ],
                ..Default::default()
            }),
            tags: HashMap::new(),
            ..Default::default()
        };

        let op_result = network_client
            .create_or_update_network_security_group(&resource_group_name, &nsg_name, &nsg)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create Network Security Group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        op_result
            .wait_for_operation_completion(&*lro_client, "CreateNetworkSecurityGroup", &nsg_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed while waiting for NSG creation".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let created_nsg = network_client
            .get_network_security_group(&resource_group_name, &nsg_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read Network Security Group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.nsg_id = created_nsg.id.clone();
        self.nsg_name = Some(nsg_name);

        info!(
            nsg_id = ?created_nsg.id,
            "Network Security Group created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingVirtualMachineScaleSets,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingVirtualMachineScaleSets,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_virtual_machine_scale_sets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        // Get subnet from network dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<AzureNetworkController>(&network_ref)?;
        let subnet_name = network
            .private_subnet_name
            .as_ref()
            .or(network.public_subnet_name.as_ref())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "No subnet available from Network".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        let vnet_name = network.vnet_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No VNet name available from Network".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let subnet_resource_group = network
            .resource_group
            .as_ref()
            .unwrap_or(&resource_group_name);
        let subnet_id = network
            .vnet_resource_id
            .as_ref()
            .map(|vnet_id| format!("{}/subnets/{}", vnet_id, subnet_name))
            .unwrap_or_else(|| {
                format!(
                    "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}/subnets/{}",
                    azure_cfg.subscription_id, subnet_resource_group, vnet_name, subnet_name
                )
            });

        let identity_id = self.identity_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Identity ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let key_vault_name = self.key_vault_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Key Vault name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let secret_name = self.machine_token_secret_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Machine token secret name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cluster_id = self.horizon_cluster_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon cluster ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let api_url = self.horizon_api_url.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon API URL not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let template_inputs = config.template_inputs.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "ContainerCluster is missing template_inputs (stamp_template_inputs did not run)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let capacity_group_prefix = format!("{}-{}-", ctx.resource_prefix, config.id);
        let cloud_init = self.generate_cloud_init(
            cluster_id,
            api_url,
            &template_inputs.horizond_download_base_url,
            key_vault_name,
            secret_name,
            &capacity_group_prefix,
            template_inputs.monitoring_logs_endpoint.as_deref(),
            template_inputs.monitoring_metrics_endpoint.as_deref(),
            self.otlp_auth_secret_name.as_deref(),
            self.otlp_metrics_auth_secret_name.as_deref(),
        );
        let custom_data = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            cloud_init.as_bytes(),
        );

        info!(
            capacity_groups = config.capacity_groups.len(),
            "Creating Virtual Machine Scale Sets for capacity groups"
        );

        for group in &config.capacity_groups {
            let vmss_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);

            let vm_sku = group.instance_type.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Capacity group '{}': instance_type not set (should be resolved by preflights)",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(
                vmss_name = %vmss_name,
                group_id = %group.group_id,
                vm_sku = %vm_sku,
                min_size = group.min_size,
                max_size = group.max_size,
                "Creating Virtual Machine Scale Set"
            );

            let location = azure_cfg
                .region
                .clone()
                .unwrap_or_else(|| "eastus".to_string());

            let vmss = VirtualMachineScaleSet {
                etag: None,
                extended_location: None,
                id: None,
                location,
                name: Some(vmss_name.clone()),
                placement: None,
                plan: None,
                sku: Some(Sku {
                    name: Some(vm_sku.clone()),
                    capacity: Some(group.min_size as i64),
                    ..Default::default()
                }),
                system_data: None,
                tags: HashMap::new(),
                type_: None,
                zones: vec![],
                identity: Some(VirtualMachineScaleSetIdentity {
                    type_: Some(ResourceIdentityType::UserAssigned),
                    user_assigned_identities: [(
                        identity_id.clone(),
                        UserAssignedIdentitiesValue::default(),
                    )]
                    .into_iter()
                    .collect(),
                    ..Default::default()
                }),
                properties: Some(VirtualMachineScaleSetProperties {
                    virtual_machine_profile: Some(VirtualMachineScaleSetVmProfile {
                        os_profile: Some(VirtualMachineScaleSetOsProfile {
                            computer_name_prefix: Some(format!("{}-{}", ctx.resource_prefix, group.group_id)),
                            admin_username: Some("azureuser".to_string()),
                            custom_data: Some(custom_data.clone()),
                            linux_configuration: Some(LinuxConfiguration {
                                disable_password_authentication: Some(true),
                                ssh: Some(SshConfiguration {
                                    public_keys: vec![],
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                        storage_profile: Some(VirtualMachineScaleSetStorageProfile {
                            image_reference: Some(ImageReference {
                                publisher: Some("Canonical".to_string()),
                                offer: Some("0001-com-ubuntu-server-jammy".to_string()),
                                sku: Some("22_04-lts-gen2".to_string()),
                                version: Some("latest".to_string()),
                                ..Default::default()
                            }),
                            os_disk: Some(VirtualMachineScaleSetOsDisk {
                                caching: Some(CachingTypes::ReadWrite),
                                create_option: DiskCreateOptionTypes::FromImage,
                                delete_option: None,
                                diff_disk_settings: None,
                                disk_size_gb: None,
                                image: None,
                                managed_disk: Some(VirtualMachineScaleSetManagedDiskParameters {
                                    storage_account_type: Some(StorageAccountTypes::PremiumLrs),
                                    ..Default::default()
                                }),
                                name: None,
                                os_type: None,
                                vhd_containers: vec![],
                                write_accelerator_enabled: None,
                            }),
                            ..Default::default()
                        }),
                        network_profile: Some(VirtualMachineScaleSetNetworkProfile {
                            health_probe: None,
                            network_api_version: None,
                            network_interface_configurations: vec![
                                VirtualMachineScaleSetNetworkConfiguration {
                                    name: format!("{}-nic", vmss_name),
                                    properties: Some(VirtualMachineScaleSetNetworkConfigurationProperties {
                                        primary: Some(true),
                                        enable_accelerated_networking: Some(false),
                                        enable_ip_forwarding: Some(false),
                                        ip_configurations: vec![
                                            VirtualMachineScaleSetIpConfiguration {
                                                name: "ipconfig1".to_string(),
                                                properties: Some(VirtualMachineScaleSetIpConfigurationProperties {
                                                    subnet: Some(ApiEntityReference {
                                                        id: Some(subnet_id.clone()),
                                                    }),
                                                    primary: Some(true),
                                                    private_ip_address_version: None,
                                                    public_ip_address_configuration: None,
                                                    application_gateway_backend_address_pools: vec![],
                                                    application_security_groups: vec![],
                                                    load_balancer_backend_address_pools: vec![],
                                                    load_balancer_inbound_nat_pools: vec![],
                                                }),
                                            }
                                        ],
                                        auxiliary_mode: None,
                                        auxiliary_sku: None,
                                        delete_option: None,
                                        disable_tcp_state_tracking: None,
                                        dns_settings: None,
                                        enable_fpga: None,
                                        network_security_group: self.nsg_id.as_ref().map(|id| SubResource {
                                            id: Some(id.clone()),
                                        }),
                                    }),
                                    tags: Default::default(),
                                }
                            ],
                        }),
                        diagnostics_profile: Some(DiagnosticsProfile {
                            boot_diagnostics: Some(BootDiagnostics {
                                enabled: Some(true),
                                storage_uri: None,
                            }),
                        }),
                        extension_profile: Some(Self::health_extension_profile()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            };

            compute_client
                .create_or_update_vmss(&resource_group_name, &vmss_name, &vmss)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create VMSS for capacity group {}",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            self.vmss_states.insert(
                group.group_id.clone(),
                VmssState {
                    group_id: group.group_id.clone(),
                    vmss_name: Some(vmss_name),
                    resource_group: Some(resource_group_name.clone()),
                    current_size: 0,
                    desired_size: group.min_size,
                    instance_type: Some(vm_sku),
                },
            );
        }

        info!("All Virtual Machine Scale Sets created");

        Ok(HandlerAction::Continue {
            state: WaitingForVmssReady,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = WaitingForVmssReady,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_vmss_ready(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let mut all_ready = true;
        let mut total_instances = 0u32;

        for state in self.vmss_states.values_mut() {
            if let (Some(vmss_name), Some(resource_group)) =
                (&state.vmss_name, &state.resource_group)
            {
                let vmss = compute_client
                    .get_vmss(resource_group, vmss_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to get VMSS {}", vmss_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                let provisioned_instances =
                    vmss.sku.and_then(|sku| sku.capacity).unwrap_or(0) as u32;

                debug!(
                    vmss_name = %vmss_name,
                    provisioned = provisioned_instances,
                    desired = state.desired_size,
                    "VMSS instance status"
                );

                if provisioned_instances < state.desired_size {
                    all_ready = false;
                }

                state.current_size = provisioned_instances;
                total_instances += provisioned_instances;
            }
        }

        if all_ready && total_instances > 0 {
            info!(
                total_instances = total_instances,
                "All VMSS instances provisioned"
            );
            Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            })
        } else if total_instances == 0 {
            // No instances expected (min_size = 0 for all groups)
            let total_min: u32 = config.capacity_groups.iter().map(|g| g.min_size).sum();
            if total_min == 0 {
                info!("No instances required (all groups have min_size=0)");
                Ok(HandlerAction::Continue {
                    state: Ready,
                    suggested_delay: None,
                })
            } else {
                self.boot_check_iterations += 1;

                if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
                    let boot_log =
                        Self::collect_serial_console_log(&*compute_client, &self.vmss_states).await;
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "No VMSS instances appeared after {} iterations (~15 minutes).\n{}",
                            BOOT_DIAG_TIMEOUT_ITERATIONS,
                            super::summarize_boot_log(&boot_log.unwrap_or_default()),
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }

                debug!("Waiting for instances to provision");
                Ok(HandlerAction::Continue {
                    state: WaitingForVmssReady,
                    suggested_delay: Some(Duration::from_secs(30)),
                })
            }
        } else {
            self.boot_check_iterations += 1;

            if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
                let boot_log =
                    Self::collect_serial_console_log(&*compute_client, &self.vmss_states).await;
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "VMSS instances did not reach desired capacity after {} iterations (~15 minutes).\n{}",
                        BOOT_DIAG_TIMEOUT_ITERATIONS,
                        super::summarize_boot_log(&boot_log.unwrap_or_default()),
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }

            debug!(
                total_instances = total_instances,
                iteration = self.boot_check_iterations,
                "Waiting for more instances to provision"
            );
            Ok(HandlerAction::Continue {
                state: WaitingForVmssReady,
                suggested_delay: Some(Duration::from_secs(30)),
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
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        debug!(
            cluster_id = %config.id,
            "Azure ContainerCluster ready, checking health"
        );

        // Periodic health check - verify VMSS exist and update instance counts
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

        for state in self.vmss_states.values_mut() {
            if let (Some(vmss_name), Some(resource_group)) =
                (&state.vmss_name, &state.resource_group)
            {
                let vmss = compute_client
                    .get_vmss(resource_group, vmss_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to get VMSS during health check".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                let provisioned_instances =
                    vmss.sku.and_then(|sku| sku.capacity).unwrap_or(0) as u32;

                state.current_size = provisioned_instances;
            }
        }

        // TODO: Poll Horizon capacity plan API every 60s and adjust VMSS capacity
        // let capacity_plan = horizon_client.get_capacity_plan(cluster_id).await?;
        // for group in capacity_plan.groups {
        //     if let Some(state) = self.vmss_states.get_mut(&group.group_id) {
        //         if state.current_size != group.desired_machines {
        //             compute_client.scale_vmss(
        //                 resource_group, vmss_name, group.desired_machines
        //             ).await?;
        //         }
        //     }
        // }

        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(60)),
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
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        info!(cluster_id = %config.id, "Azure ContainerCluster update requested");
        self.boot_check_iterations = 0;
        Ok(HandlerAction::Continue {
            state: UpdatingOtlpSecrets,
            suggested_delay: None,
        })
    }

    #[handler(
        state = UpdatingOtlpSecrets,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_otlp_secrets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let kv_client = ctx
            .service_provider
            .get_azure_key_vault_secrets_client(azure_cfg)?;

        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let vault_name = self.key_vault_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Key Vault name not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let vault_base_url = format!("https://{}.vault.azure.net", vault_name);

            let logs_secret_name = "ALIEN-OTLP-AUTH-HEADER".to_string();
            kv_client
                .set_secret(
                    vault_base_url.clone(),
                    logs_secret_name.clone(),
                    SecretSetParameters {
                        attributes: None,
                        content_type: Some("text/plain".to_string()),
                        tags: HashMap::new(),
                        value: monitoring.logs_auth_header.clone(),
                    },
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update OTLP logs auth header in Key Vault".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
            self.otlp_auth_secret_name = Some(logs_secret_name);

            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_name = "ALIEN-OTLP-METRICS-AUTH-HEADER".to_string();
                kv_client
                    .set_secret(
                        vault_base_url.clone(),
                        metrics_secret_name.clone(),
                        SecretSetParameters {
                            attributes: None,
                            content_type: Some("text/plain".to_string()),
                            tags: HashMap::new(),
                            value: metrics_auth_header.clone(),
                        },
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to update OTLP metrics auth header in Key Vault"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_name);
            } else {
                self.otlp_metrics_auth_secret_name = None;
            }

            info!("OTLP secrets updated in Key Vault");
        } else {
            self.otlp_auth_secret_name = None;
            self.otlp_metrics_auth_secret_name = None;
        }

        Ok(HandlerAction::Continue {
            state: SyncingHorizonCapacityGroups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = SyncingHorizonCapacityGroups,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn syncing_horizon_capacity_groups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let cluster_id = self.horizon_cluster_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon cluster ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let api_url = self.horizon_api_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon API URL not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let horizon_config = match &ctx.deployment_config.compute_backend {
            Some(ComputeBackend::Horizon(h)) => h,
            None => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "ContainerCluster requires Horizon backend".to_string(),
                    resource_id: Some(config.id.clone()),
                }))
            }
        };
        let cluster_config = horizon_config.clusters.get(&config.id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("No Horizon cluster config for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let horizon_client = create_horizon_client(&api_url, &cluster_config.management_token)
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to create Horizon client: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        let horizon_groups = to_horizon_capacity_groups(&config.capacity_groups, &config.id)?;
        horizon_client
            .update_cluster()
            .cluster_id(&cluster_id)
            .body(&horizon_client_sdk::types::UpdateClusterRequest {
                capacity_groups: horizon_groups,
            })
            .send()
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: format!("Failed to sync Horizon capacity groups: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        info!(cluster_id = %cluster_id, "Synced Horizon capacity groups");
        self.horizon_ready = true;
        Ok(HandlerAction::Continue {
            state: ResizingExistingVmss,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ResizingExistingVmss,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn resizing_existing_vmss(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

        for group in &config.capacity_groups {
            if let Some(state) = self.vmss_states.get_mut(&group.group_id) {
                if let (Some(vmss_name), Some(rg)) =
                    (state.vmss_name.clone(), state.resource_group.clone())
                {
                    let mut vmss = compute_client.get_vmss(&rg, &vmss_name).await.context(
                        ErrorData::CloudPlatformError {
                            message: format!("Failed to get VMSS {}", vmss_name),
                            resource_id: Some(config.id.clone()),
                        },
                    )?;
                    let mut sku = vmss.sku.take().unwrap_or_default();
                    sku.capacity = Some(group.min_size as i64);
                    if sku.name.is_none() {
                        sku.name = state
                            .instance_type
                            .clone()
                            .or_else(|| group.instance_type.clone());
                    }
                    vmss.sku = Some(sku);
                    compute_client
                        .create_or_update_vmss(&rg, &vmss_name, &vmss)
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to update VMSS {}", vmss_name),
                            resource_id: Some(config.id.clone()),
                        })?;
                    state.desired_size = group.min_size;
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingRemovedVmss,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRemovedVmss,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn deleting_removed_vmss(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let removed_group_ids: Vec<String> = self
            .vmss_states
            .keys()
            .filter(|id| !config.capacity_groups.iter().any(|g| &g.group_id == *id))
            .cloned()
            .collect();

        for group_id in &removed_group_ids {
            self.delete_capacity_group(ctx, group_id, &config.id)
                .await?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingNewVmss,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNewVmss,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_vmss(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let new_groups: Vec<CapacityGroup> = config
            .capacity_groups
            .iter()
            .filter(|g| !self.vmss_states.contains_key(&g.group_id))
            .cloned()
            .collect();

        if !new_groups.is_empty() {
            let resource_group_name =
                crate::infra_requirements::azure_utils::get_resource_group_name(ctx.state)?;
            let azure_cfg = ctx.get_azure_config()?;
            let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

            let network_ref =
                ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
            let network = ctx.require_dependency::<AzureNetworkController>(&network_ref)?;
            let subnet_name = network
                .private_subnet_name
                .as_ref()
                .or(network.public_subnet_name.as_ref())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "No subnet".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            let vnet_name = network.vnet_name.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "No VNet".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let subnet_rg = network
                .resource_group
                .as_ref()
                .unwrap_or(&resource_group_name);
            let subnet_id = network.vnet_resource_id.as_ref()
                .map(|id| format!("{}/subnets/{}", id, subnet_name))
                .unwrap_or_else(|| format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Network/virtualNetworks/{}/subnets/{}", azure_cfg.subscription_id, subnet_rg, vnet_name, subnet_name));

            let identity_id = self.identity_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Identity ID not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let key_vault_name = self.key_vault_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Key Vault not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let secret_name = self.machine_token_secret_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Machine token not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let cluster_id = self.horizon_cluster_id.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Horizon cluster ID not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let api_url_str = self.horizon_api_url.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Horizon API URL not set".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let template_inputs: TemplateInputs =
                config.template_inputs.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Missing template_inputs".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
            let otlp_auth = self.otlp_auth_secret_name.clone();
            let otlp_metrics_auth = self.otlp_metrics_auth_secret_name.clone();
            let nsg_id = self.nsg_id.clone();
            let location = azure_cfg
                .region
                .clone()
                .unwrap_or_else(|| "eastus".to_string());
            let cap_prefix = format!("{}-{}-", ctx.resource_prefix, config.id);
            let cloud_init = self.generate_cloud_init(
                &cluster_id,
                &api_url_str,
                &template_inputs.horizond_download_base_url,
                &key_vault_name,
                &secret_name,
                &cap_prefix,
                template_inputs.monitoring_logs_endpoint.as_deref(),
                template_inputs.monitoring_metrics_endpoint.as_deref(),
                otlp_auth.as_deref(),
                otlp_metrics_auth.as_deref(),
            );
            let custom_data = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                cloud_init.as_bytes(),
            );

            for group in &new_groups {
                if self.vmss_states.contains_key(&group.group_id) {
                    continue;
                }

                let vm_sku = group.instance_type.clone().ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: format!("Group '{}': instance_type not set", group.group_id),
                        resource_id: Some(config.id.clone()),
                    })
                })?;
                let vmss_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);

                let vmss = VirtualMachineScaleSet {
                    etag: None, extended_location: None, id: None, location: location.clone(),
                    name: Some(vmss_name.clone()), placement: None, plan: None,
                    sku: Some(Sku { name: Some(vm_sku.clone()), capacity: Some(group.min_size as i64), ..Default::default() }),
                    system_data: None, tags: HashMap::new(), type_: None, zones: vec![],
                    identity: Some(VirtualMachineScaleSetIdentity {
                        type_: Some(ResourceIdentityType::UserAssigned),
                        user_assigned_identities: [(identity_id.clone(), UserAssignedIdentitiesValue::default())].into_iter().collect(),
                        ..Default::default()
                    }),
                    properties: Some(VirtualMachineScaleSetProperties {
                        virtual_machine_profile: Some(VirtualMachineScaleSetVmProfile {
                            os_profile: Some(VirtualMachineScaleSetOsProfile {
                                computer_name_prefix: Some(format!("{}-{}", ctx.resource_prefix, group.group_id)),
                                admin_username: Some("azureuser".to_string()),
                                custom_data: Some(custom_data.clone()),
                                linux_configuration: Some(LinuxConfiguration { disable_password_authentication: Some(true), ssh: Some(SshConfiguration { public_keys: vec![] }), ..Default::default() }),
                                ..Default::default()
                            }),
                            storage_profile: Some(VirtualMachineScaleSetStorageProfile {
                                image_reference: Some(ImageReference { publisher: Some("Canonical".to_string()), offer: Some("0001-com-ubuntu-server-jammy".to_string()), sku: Some("22_04-lts-gen2".to_string()), version: Some("latest".to_string()), ..Default::default() }),
                                os_disk: Some(VirtualMachineScaleSetOsDisk {
                                    caching: Some(CachingTypes::ReadWrite),
                                    create_option: DiskCreateOptionTypes::FromImage,
                                    delete_option: None,
                                    diff_disk_settings: None,
                                    disk_size_gb: None,
                                    image: None,
                                    managed_disk: Some(VirtualMachineScaleSetManagedDiskParameters { storage_account_type: Some(StorageAccountTypes::PremiumLrs), ..Default::default() }),
                                    name: None,
                                    os_type: None,
                                    vhd_containers: vec![],
                                    write_accelerator_enabled: None,
                                }),
                                ..Default::default()
                            }),
                            network_profile: Some(VirtualMachineScaleSetNetworkProfile {
                                health_probe: None, network_api_version: None,
                                network_interface_configurations: vec![VirtualMachineScaleSetNetworkConfiguration {
                                    name: format!("{}-nic", vmss_name),
                                    properties: Some(VirtualMachineScaleSetNetworkConfigurationProperties {
                                        primary: Some(true),
                                        enable_accelerated_networking: Some(false),
                                        enable_ip_forwarding: Some(false),
                                        ip_configurations: vec![VirtualMachineScaleSetIpConfiguration {
                                            name: "ipconfig1".to_string(),
                                            properties: Some(VirtualMachineScaleSetIpConfigurationProperties {
                                                subnet: Some(ApiEntityReference { id: Some(subnet_id.clone()) }),
                                                primary: Some(true),
                                                private_ip_address_version: None,
                                                public_ip_address_configuration: None,
                                                application_gateway_backend_address_pools: vec![],
                                                application_security_groups: vec![],
                                                load_balancer_backend_address_pools: vec![],
                                                load_balancer_inbound_nat_pools: vec![],
                                            }),
                                        }],
                                        network_security_group: nsg_id.as_ref().map(|id| SubResource { id: Some(id.clone()) }),
                                        auxiliary_mode: None,
                                        auxiliary_sku: None,
                                        delete_option: None,
                                        disable_tcp_state_tracking: None,
                                        dns_settings: None,
                                        enable_fpga: None,
                                    }),
                                    tags: Default::default(),
                                }],
                            }),
                            diagnostics_profile: Some(DiagnosticsProfile { boot_diagnostics: Some(BootDiagnostics { enabled: Some(true), storage_uri: None }) }),
                            extension_profile: Some(Self::health_extension_profile()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                };

                compute_client
                    .create_or_update_vmss(&resource_group_name, &vmss_name, &vmss)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create VMSS for group '{}'", group.group_id),
                        resource_id: Some(config.id.clone()),
                    })?;

                self.vmss_states.insert(
                    group.group_id.clone(),
                    VmssState {
                        group_id: group.group_id.clone(),
                        vmss_name: Some(vmss_name),
                        resource_group: Some(resource_group_name.clone()),
                        current_size: 0,
                        desired_size: group.min_size,
                        instance_type: Some(vm_sku),
                    },
                );
                info!(group_id = %group.group_id, "Added new VMSS for capacity group");
            }
        }

        self.new_groups_pending_ready = new_groups.iter().map(|g| g.group_id.clone()).collect();
        Ok(HandlerAction::Continue {
            state: WaitingForNewVmssReady,
            suggested_delay: if self.new_groups_pending_ready.is_empty() {
                None
            } else {
                Some(Duration::from_secs(30))
            },
        })
    }

    #[handler(
        state = WaitingForNewVmssReady,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_new_vmss_ready(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.new_groups_pending_ready.is_empty() {
            return Ok(HandlerAction::Continue {
                state: UpdatingVmssModelForRollingUpgrade,
                suggested_delay: None,
            });
        }
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let total_new_min: u32 = self
            .new_groups_pending_ready
            .iter()
            .filter_map(|id| config.capacity_groups.iter().find(|g| &g.group_id == id))
            .map(|g| g.min_size)
            .sum();
        if total_new_min == 0 {
            self.new_groups_pending_ready.clear();
            return Ok(HandlerAction::Continue {
                state: UpdatingVmssModelForRollingUpgrade,
                suggested_delay: None,
            });
        }

        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
        let mut all_ready = true;
        let mut total = 0u32;
        let new_ids = self.new_groups_pending_ready.clone();
        for group_id in &new_ids {
            if let Some(state) = self.vmss_states.get_mut(group_id) {
                if let (Some(vmss_name), Some(rg)) = (&state.vmss_name, &state.resource_group) {
                    let vmss = compute_client.get_vmss(rg, vmss_name).await.context(
                        ErrorData::CloudPlatformError {
                            message: format!("Failed to get VMSS {}", vmss_name),
                            resource_id: Some(config.id.clone()),
                        },
                    )?;
                    let provisioned = vmss.sku.and_then(|s| s.capacity).unwrap_or(0) as u32;
                    if provisioned < state.desired_size {
                        all_ready = false;
                    }
                    state.current_size = provisioned;
                    total += provisioned;
                }
            }
        }
        if all_ready && total > 0 {
            info!(total, "All new VMSS instances provisioned");
            self.new_groups_pending_ready.clear();
            return Ok(HandlerAction::Continue {
                state: UpdatingVmssModelForRollingUpgrade,
                suggested_delay: None,
            });
        }
        self.boot_check_iterations += 1;
        if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
            let new_states: HashMap<String, VmssState> = self
                .vmss_states
                .iter()
                .filter(|(id, _)| self.new_groups_pending_ready.contains(id))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let boot_log = Self::collect_serial_console_log(&*compute_client, &new_states).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "New VMSS instances not ready after ~15 min.\n{}",
                    super::summarize_boot_log(&boot_log.unwrap_or_default())
                ),
                resource_id: Some(config.id.clone()),
            }));
        }
        Ok(HandlerAction::Continue {
            state: WaitingForNewVmssReady,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── ROLLING UPDATE STATES ─────────────────────

    #[handler(
        state = UpdatingVmssModelForRollingUpgrade,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn updating_vmss_model_for_rolling_upgrade(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let previous_config = ctx.previous_resource_config::<ContainerCluster>().ok();
        let template_inputs_changed = match previous_config {
            Some(prev) => prev.template_inputs != config.template_inputs,
            None => true,
        };

        if !template_inputs_changed {
            info!("Template inputs unchanged, skipping rolling update");
            self.rolling_update_triggered = false;
            return Ok(HandlerAction::Continue {
                state: TriggeringRollingUpgrade,
                suggested_delay: None,
            });
        }

        self.rolling_update_poll_iterations = 0;

        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

        let cluster_id = self.horizon_cluster_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon cluster ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let api_url = self.horizon_api_url.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Horizon API URL not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let template_inputs = config.template_inputs.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Missing template_inputs".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let key_vault_name = self.key_vault_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Key Vault not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let secret_name = self.machine_token_secret_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Machine token not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let otlp_auth = self.otlp_auth_secret_name.clone();
        let cap_prefix = format!("{}-{}-", ctx.resource_prefix, config.id);

        let cloud_init = self.generate_cloud_init(
            &cluster_id,
            &api_url,
            &template_inputs.horizond_download_base_url,
            &key_vault_name,
            &secret_name,
            &cap_prefix,
            template_inputs.monitoring_logs_endpoint.as_deref(),
            template_inputs.monitoring_metrics_endpoint.as_deref(),
            otlp_auth.as_deref(),
            self.otlp_metrics_auth_secret_name.as_deref(),
        );
        let custom_data = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            cloud_init.as_bytes(),
        );

        let vmss_ids: Vec<(String, String, String)> = self
            .vmss_states
            .iter()
            .filter_map(|(gid, s)| {
                Some((gid.clone(), s.vmss_name.clone()?, s.resource_group.clone()?))
            })
            .collect();

        for (group_id, vmss_name, rg) in &vmss_ids {
            info!(vmss_name = %vmss_name, group_id = %group_id, "Updating VMSS model for rolling upgrade");

            let mut vmss = compute_client.get_vmss(rg, vmss_name).await.context(
                ErrorData::CloudPlatformError {
                    message: format!("Failed to get VMSS {}", vmss_name),
                    resource_id: Some(config.id.clone()),
                },
            )?;

            if let Some(ref mut props) = vmss.properties {
                if let Some(ref mut vm_profile) = props.virtual_machine_profile {
                    if let Some(ref mut os_profile) = vm_profile.os_profile {
                        os_profile.custom_data = Some(custom_data.clone());
                    }
                    vm_profile.extension_profile = Some(Self::health_extension_profile());
                }
                use alien_azure_clients::azure::models::compute_rp::{
                    RollingUpgradePolicy, UpgradeMode, UpgradePolicy,
                };
                props.upgrade_policy = Some(UpgradePolicy {
                    mode: Some(UpgradeMode::Rolling),
                    rolling_upgrade_policy: Some(RollingUpgradePolicy {
                        max_batch_instance_percent: Some(20),
                        max_unhealthy_instance_percent: Some(20),
                        max_unhealthy_upgraded_instance_percent: Some(20),
                        pause_time_between_batches: Some("PT0S".to_string()),
                        max_surge: Some(true),
                        ..Default::default()
                    }),
                    automatic_os_upgrade_policy: None,
                });
                props.provisioning_state = None;
            }

            compute_client
                .create_or_update_vmss(rg, vmss_name, &vmss)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to update VMSS model for {}", vmss_name),
                    resource_id: Some(config.id.clone()),
                })?;
        }

        self.rolling_update_triggered = true;
        Ok(HandlerAction::Continue {
            state: TriggeringRollingUpgrade,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = TriggeringRollingUpgrade,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn triggering_rolling_upgrade(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if !self.rolling_update_triggered {
            return Ok(HandlerAction::Continue {
                state: WaitingForRollingUpgradeComplete,
                suggested_delay: None,
            });
        }

        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        for (_group_id, state) in &self.vmss_states {
            if let (Some(vmss_name), Some(rg)) = (&state.vmss_name, &state.resource_group) {
                info!(vmss_name = %vmss_name, "Triggering VMSS rolling upgrade");

                let op = compute_client
                    .start_vmss_rolling_upgrade(rg, vmss_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to start rolling upgrade for VMSS {}", vmss_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                op.wait_for_operation_completion(
                    &*lro_client,
                    "StartVmssRollingUpgrade",
                    vmss_name,
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Rolling upgrade LRO failed for VMSS {}", vmss_name),
                    resource_id: Some(config.id.clone()),
                })?;
            }
        }

        self.rolling_update_poll_iterations = 0;
        Ok(HandlerAction::Continue {
            state: WaitingForRollingUpgradeComplete,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = WaitingForRollingUpgradeComplete,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_rolling_upgrade_complete(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if !self.rolling_update_triggered {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

        const ROLLING_UPDATE_TIMEOUT: u32 = 60;

        let mut all_complete = true;

        for (_group_id, state) in &self.vmss_states {
            if let (Some(vmss_name), Some(rg)) = (&state.vmss_name, &state.resource_group) {
                let status: RollingUpgradeLatestStatus = compute_client
                    .get_vmss_rolling_upgrade_latest(rg, vmss_name)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to get rolling upgrade status for VMSS {}",
                            vmss_name
                        ),
                        resource_id: Some(config.id.clone()),
                    })?;

                let code = status
                    .properties
                    .as_ref()
                    .and_then(|p| p.running_status.as_ref())
                    .and_then(|r| r.code.as_deref())
                    .unwrap_or("Unknown");

                match code {
                    "Completed" => {
                        debug!(vmss_name = %vmss_name, "Rolling upgrade completed");
                    }
                    "Faulted" => {
                        return Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: format!("Rolling upgrade for VMSS {} faulted", vmss_name),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    "Cancelled" => {
                        return Err(AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "Rolling upgrade for VMSS {} was cancelled",
                                vmss_name
                            ),
                            resource_id: Some(config.id.clone()),
                        }));
                    }
                    _ => {
                        debug!(vmss_name = %vmss_name, status = %code, "Rolling upgrade in progress");
                        all_complete = false;
                    }
                }
            }
        }

        if all_complete {
            info!("All VMSS rolling upgrades complete");
            self.rolling_update_triggered = false;
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        self.rolling_update_poll_iterations += 1;
        if self.rolling_update_poll_iterations >= ROLLING_UPDATE_TIMEOUT {
            let boot_log =
                Self::collect_serial_console_log(&*compute_client, &self.vmss_states).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Rolling upgrade did not complete after {} iterations (~30 min).\n{}",
                    ROLLING_UPDATE_TIMEOUT,
                    super::summarize_boot_log(&boot_log.unwrap_or_default()),
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        Ok(HandlerAction::Continue {
            state: WaitingForRollingUpgradeComplete,
            suggested_delay: Some(Duration::from_secs(30)),
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
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(
            cluster_id = %config.id,
            "Starting Azure ContainerCluster deletion"
        );

        Ok(HandlerAction::Continue {
            state: DeletingVirtualMachineScaleSets,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingVirtualMachineScaleSets,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_virtual_machine_scale_sets(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;

        for state in self.vmss_states.values() {
            if let (Some(vmss_name), Some(resource_group)) =
                (&state.vmss_name, &state.resource_group)
            {
                info!(vmss_name = %vmss_name, "Deleting Virtual Machine Scale Set");

                match compute_client.delete_vmss(resource_group, vmss_name).await {
                    Ok(_) => info!(vmss_name = %vmss_name, "VMSS deleted"),
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(vmss_name = %vmss_name, "VMSS already deleted")
                    }
                    Err(e) => {
                        return Err(e).context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete VMSS {}", vmss_name),
                            resource_id: None,
                        })
                    }
                }
            }
        }

        self.vmss_states.clear();

        Ok(HandlerAction::Continue {
            state: DeletingNetworkSecurityGroup,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = DeletingNetworkSecurityGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_network_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let network_client = ctx.service_provider.get_azure_network_client(azure_cfg)?;
        let lro_client = ctx
            .service_provider
            .get_azure_long_running_operation_client(azure_cfg)?;

        if let Some(nsg_name) = &self.nsg_name {
            info!(nsg_name = %nsg_name, "Deleting Network Security Group");

            match network_client
                .delete_network_security_group(&resource_group_name, nsg_name)
                .await
            {
                Ok(op_result) => {
                    op_result
                        .wait_for_operation_completion(
                            &*lro_client,
                            "DeleteNetworkSecurityGroup",
                            nsg_name,
                        )
                        .await
                        .ok();
                    info!(nsg_name = %nsg_name, "NSG deleted");
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(nsg_name = %nsg_name, "NSG already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete NSG {}", nsg_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.nsg_name = None;
        self.nsg_id = None;

        Ok(HandlerAction::Continue {
            state: DeletingMachineToken,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingMachineToken,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_machine_token(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let kv_client = ctx
            .service_provider
            .get_azure_key_vault_secrets_client(azure_cfg)?;

        if let (Some(vault_name), Some(secret_name)) =
            (&self.key_vault_name, &self.machine_token_secret_name)
        {
            info!(
                vault_name = %vault_name,
                secret_name = %secret_name,
                "Deleting machine token secret"
            );

            let vault_base_url = format!("https://{}.vault.azure.net", vault_name);

            match kv_client
                .delete_secret(vault_base_url, secret_name.clone())
                .await
            {
                Ok(_) => info!(secret_name = %secret_name, "Secret deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(secret_name = %secret_name, "Secret already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete secret {}", secret_name),
                        resource_id: None,
                    })
                }
            }
        }

        // Delete OTLP auth header secrets if they were created
        if let Some(vault_name) = &self.key_vault_name {
            let vault_base_url = format!("https://{}.vault.azure.net", vault_name);
            for otlp_secret_name in [
                self.otlp_auth_secret_name.take(),
                self.otlp_metrics_auth_secret_name.take(),
            ]
            .into_iter()
            .flatten()
            {
                info!(vault_name = %vault_name, secret_name = %otlp_secret_name, "Deleting OTLP secret");
                match kv_client
                    .delete_secret(vault_base_url.clone(), otlp_secret_name.clone())
                    .await
                {
                    Ok(_) => info!(secret_name = %otlp_secret_name, "OTLP secret deleted"),
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(secret_name = %otlp_secret_name, "OTLP secret already deleted")
                    }
                    Err(e) => {
                        return Err(e).context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete OTLP secret {}", otlp_secret_name),
                            resource_id: None,
                        })
                    }
                }
            }
        }

        self.key_vault_name = None;
        self.machine_token_secret_name = None;
        self.otlp_auth_secret_name = None;
        self.otlp_metrics_auth_secret_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingRoleAssignments,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRoleAssignments,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let auth_client = ctx
            .service_provider
            .get_azure_authorization_client(azure_cfg)?;
        let mut remaining = Vec::new();

        for role_assignment_id in &self.role_assignment_ids {
            match auth_client
                .delete_role_assignment_by_id(role_assignment_id.clone())
                .await
            {
                Ok(_) => info!(role_assignment_id = %role_assignment_id, "Role assignment deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(role_assignment_id = %role_assignment_id, "Role assignment already deleted")
                }
                Err(e) => {
                    remaining.push(role_assignment_id.clone());
                    warn!(role_assignment_id = %role_assignment_id, "Failed to delete role assignment: {}", e);
                }
            }
        }

        self.role_assignment_ids = remaining;

        Ok(HandlerAction::Continue {
            state: DeletingManagedIdentity,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagedIdentity,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_managed_identity(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let azure_cfg = ctx.get_azure_config()?;
        let resource_group_name = azure_utils::get_resource_group_name(ctx.state)?;
        let identity_client = ctx
            .service_provider
            .get_azure_managed_identity_client(azure_cfg)?;

        if let Some(identity_id) = &self.identity_id {
            // Extract identity name from ID
            let identity_name = identity_id.split('/').last().unwrap_or("unknown");

            info!(
                identity_name = %identity_name,
                "Deleting managed identity"
            );

            match identity_client
                .delete_user_assigned_identity(&resource_group_name, identity_name)
                .await
            {
                Ok(_) => info!(identity_name = %identity_name, "Identity deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(identity_name = %identity_name, "Identity already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete identity {}", identity_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.identity_id = None;
        self.identity_principal_id = None;
        self.identity_client_id = None;

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
        let cluster_id = self.horizon_cluster_id.as_ref()?;

        let capacity_group_statuses: Vec<_> = self
            .vmss_states
            .values()
            .map(|state| CapacityGroupStatus {
                group_id: state.group_id.clone(),
                current_machines: state.current_size,
                desired_machines: state.desired_size,
                instance_type: state.instance_type.clone().unwrap_or_default(),
            })
            .collect();

        let total_machines: u32 = capacity_group_statuses
            .iter()
            .map(|s| s.current_machines)
            .sum();

        Some(ResourceOutputs::new(ContainerClusterOutputs {
            cluster_id: cluster_id.clone(),
            horizon_ready: self.horizon_ready,
            capacity_group_statuses,
            total_machines,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }
}

impl AzureContainerClusterController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(cluster_id: &str, capacity_groups: Vec<(&str, u32)>) -> Self {
        let mut vmss_states = HashMap::new();
        for (group_id, size) in capacity_groups {
            vmss_states.insert(
                group_id.to_string(),
                VmssState {
                    group_id: group_id.to_string(),
                    vmss_name: Some(format!("test-vmss-{}", group_id)),
                    resource_group: Some("test-rg".to_string()),
                    current_size: size,
                    desired_size: size,
                    instance_type: Some("Standard_B2s".to_string()),
                },
            );
        }

        Self {
            state: AzureContainerClusterState::Ready,
            identity_id: Some("/subscriptions/test/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity".to_string()),
            identity_principal_id: Some("test-principal".to_string()),
            identity_client_id: Some("test-client".to_string()),
            key_vault_name: Some("test-kv".to_string()),
            machine_token_secret_name: Some("test-secret".to_string()),
            otlp_auth_secret_name: None,
            otlp_metrics_auth_secret_name: None,
            nsg_id: Some("/subscriptions/test/resourceGroups/test-rg/providers/Microsoft.Network/networkSecurityGroups/test-nsg".to_string()),
            nsg_name: Some("test-nsg".to_string()),
            vmss_states,
            role_assignment_ids: vec![
                "/subscriptions/test/resourceGroups/test-rg/providers/Microsoft.Authorization/roleAssignments/test-contributor".to_string(),
                "/subscriptions/test/resourceGroups/test-rg/providers/Microsoft.Authorization/roleAssignments/test-keyvault".to_string(),
            ],
            horizon_cluster_id: Some(cluster_id.to_string()),
            horizon_ready: true,
            horizon_api_url: Some("https://horizon.example.com".to_string()),
            boot_check_iterations: 0,
            new_groups_pending_ready: vec![],
            rolling_update_triggered: false,
            rolling_update_poll_iterations: 0,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_azure_clients::azure::authorization::MockAuthorizationApi;
    use alien_azure_clients::azure::compute::{
        MockVirtualMachineScaleSetsApi, RollingUpgradeLatestStatus, RollingUpgradeRunningStatus,
        RollingUpgradeStatusProperties,
    };
    use alien_azure_clients::azure::keyvault::MockKeyVaultSecretsApi;
    use alien_azure_clients::azure::long_running_operation::{
        MockLongRunningOperationApi, OperationResult,
    };
    use alien_azure_clients::azure::managed_identity::MockManagedIdentityApi;
    use alien_azure_clients::azure::models::compute_rp::{Sku, VirtualMachineScaleSet};
    use alien_azure_clients::azure::models::managed_identity::{
        AzureCoreUuid, UserAssignedIdentityProperties,
    };
    use alien_azure_clients::azure::models::network_security_group::NetworkSecurityGroup;
    use alien_azure_clients::azure::models::secrets::SecretBundle;
    use alien_azure_clients::azure::network::MockNetworkApi;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::NetworkSettings;
    use alien_core::{
        CapacityGroup, ComputeBackend, ContainerCluster, EnvironmentVariable,
        EnvironmentVariableType, EnvironmentVariablesSnapshot, HorizonClusterConfig, HorizonConfig,
        Network, ResourceStatus,
    };
    use httpmock::prelude::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    fn test_vmss() -> VirtualMachineScaleSet {
        VirtualMachineScaleSet {
            etag: None,
            extended_location: None,
            id: None,
            identity: None,
            location: "eastus".to_string(),
            name: None,
            placement: None,
            plan: None,
            properties: None,
            sku: None,
            system_data: None,
            tags: HashMap::new(),
            type_: None,
            zones: vec![],
        }
    }

    fn setup_mock_provider(
        identity: Arc<MockManagedIdentityApi>,
        authorization: Arc<MockAuthorizationApi>,
        key_vault: Arc<MockKeyVaultSecretsApi>,
        network: Arc<MockNetworkApi>,
        compute: Arc<MockVirtualMachineScaleSetsApi>,
        lro: Arc<MockLongRunningOperationApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_azure_managed_identity_client()
            .returning(move |_| Ok(identity.clone()));
        provider
            .expect_get_azure_authorization_client()
            .returning(move |_| Ok(authorization.clone()));
        provider
            .expect_get_azure_key_vault_secrets_client()
            .returning(move |_| Ok(key_vault.clone()));
        provider
            .expect_get_azure_network_client()
            .returning(move |_| Ok(network.clone()));
        provider
            .expect_get_azure_compute_client()
            .returning(move |_| Ok(compute.clone()));
        provider
            .expect_get_azure_long_running_operation_client()
            .returning(move |_| Ok(lro.clone()));
        Arc::new(provider)
    }

    fn mock_azure_clients_for_create_delete() -> (
        Arc<MockManagedIdentityApi>,
        Arc<MockAuthorizationApi>,
        Arc<MockKeyVaultSecretsApi>,
        Arc<MockNetworkApi>,
        Arc<MockVirtualMachineScaleSetsApi>,
        Arc<MockLongRunningOperationApi>,
    ) {
        let mut identity = MockManagedIdentityApi::new();
        let mut authorization = MockAuthorizationApi::new();
        let mut key_vault = MockKeyVaultSecretsApi::new();
        let mut network = MockNetworkApi::new();
        let mut compute = MockVirtualMachineScaleSetsApi::new();
        let lro = Arc::new(MockLongRunningOperationApi::new());

        identity
            .expect_get_user_assigned_identity()
            .returning(|_, _| {
                Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceNotFound {
                        resource_type: "ManagedIdentity".to_string(),
                        resource_name: "missing".to_string(),
                    },
                ))
            });
        identity
            .expect_create_or_update_user_assigned_identity()
            .returning(|_, _, _| {
                Ok(Identity {
                    id: Some("/subscriptions/test/resourceGroups/test/providers/Microsoft.ManagedIdentity/userAssignedIdentities/test-identity".to_string()),
                    location: "eastus".to_string(),
                    name: Some("test-identity".to_string()),
                    properties: Some(UserAssignedIdentityProperties {
                        principal_id: Some(AzureCoreUuid(Uuid::new_v4())),
                        client_id: Some(AzureCoreUuid(Uuid::new_v4())),
                        ..Default::default()
                    }),
                    system_data: None,
                    tags: HashMap::new(),
                    type_: None,
                })
            });
        identity
            .expect_delete_user_assigned_identity()
            .returning(|_, _| Ok(()));

        authorization
            .expect_build_resource_group_role_assignment_id()
            .returning(|_, _| "/subscriptions/test/resourceGroups/test/providers/Microsoft.Authorization/roleAssignments/test-contributor".to_string());
        authorization
            .expect_build_resource_role_assignment_id()
            .returning(|_, _, _, _, _, _| "/subscriptions/test/resourceGroups/test/providers/Microsoft.Authorization/roleAssignments/test-keyvault".to_string());
        authorization
            .expect_create_or_update_role_assignment_by_id()
            .returning(|_, _| Ok(RoleAssignment::default()));
        authorization
            .expect_delete_role_assignment_by_id()
            .returning(|_| Ok(None));

        key_vault
            .expect_set_secret()
            .returning(|_, _, _| Ok(SecretBundle::default()));
        key_vault
            .expect_delete_secret()
            .returning(|_, _| Ok(SecretBundle::default()));

        network
            .expect_create_or_update_network_security_group()
            .returning(|_, _, _| Ok(OperationResult::Completed(NetworkSecurityGroup::default())));
        network
            .expect_get_network_security_group()
            .returning(|_, _| {
                Ok(NetworkSecurityGroup {
                    id: Some("/subscriptions/test/resourceGroups/test/providers/Microsoft.Network/networkSecurityGroups/test-nsg".to_string()),
                    ..Default::default()
                })
            });
        network
            .expect_delete_network_security_group()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        compute
            .expect_create_or_update_vmss()
            .returning(|_, _, _| Ok(OperationResult::Completed(test_vmss())));
        compute.expect_get_vmss().returning(|_, _| {
            let mut vmss = test_vmss();
            vmss.sku = Some(Sku {
                name: Some("Standard_B2s".to_string()),
                capacity: Some(1),
                ..Default::default()
            });
            Ok(vmss)
        });
        compute
            .expect_delete_vmss()
            .returning(|_, _| Ok(OperationResult::Completed(())));

        (
            Arc::new(identity),
            Arc::new(authorization),
            Arc::new(key_vault),
            Arc::new(network),
            Arc::new(compute),
            lro,
        )
    }

    fn mock_azure_clients_for_best_effort_delete() -> (
        Arc<MockManagedIdentityApi>,
        Arc<MockAuthorizationApi>,
        Arc<MockKeyVaultSecretsApi>,
        Arc<MockNetworkApi>,
        Arc<MockVirtualMachineScaleSetsApi>,
        Arc<MockLongRunningOperationApi>,
    ) {
        let mut identity = MockManagedIdentityApi::new();
        let mut authorization = MockAuthorizationApi::new();
        let mut key_vault = MockKeyVaultSecretsApi::new();
        let mut network = MockNetworkApi::new();
        let mut compute = MockVirtualMachineScaleSetsApi::new();
        let lro = Arc::new(MockLongRunningOperationApi::new());

        let not_found = || {
            AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
                resource_type: "Azure".to_string(),
                resource_name: "missing".to_string(),
            })
        };

        compute
            .expect_delete_vmss()
            .returning(move |_, _| Err(not_found()));
        network
            .expect_delete_network_security_group()
            .returning(move |_, _| Err(not_found()));
        key_vault
            .expect_delete_secret()
            .returning(move |_, _| Err(not_found()));
        authorization
            .expect_delete_role_assignment_by_id()
            .returning(move |_| Err(not_found()));
        identity
            .expect_delete_user_assigned_identity()
            .returning(move |_, _| Err(not_found()));

        (
            Arc::new(identity),
            Arc::new(authorization),
            Arc::new(key_vault),
            Arc::new(network),
            Arc::new(compute),
            lro,
        )
    }

    fn test_horizon_config(cluster_id: &str) -> ComputeBackend {
        let mut clusters = HashMap::new();
        clusters.insert(
            "compute".to_string(),
            HorizonClusterConfig {
                cluster_id: cluster_id.to_string(),
                management_token: "hm_test".to_string(),
            },
        );

        ComputeBackend::Horizon(HorizonConfig {
            url: "http://horizon.test".to_string(),
            horizond_download_base_url: "http://releases.test".to_string(),
            horizond_binary_hash: None,
            clusters,
        })
    }

    fn test_cluster() -> ContainerCluster {
        let mut cluster = ContainerCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("Standard_B2s".to_string()),
                profile: Some(alien_core::MachineProfile {
                    cpu: "2.0".to_string(),
                    memory_bytes: 4 * 1024 * 1024 * 1024,
                    ephemeral_storage_bytes: 20 * 1024 * 1024 * 1024,
                    gpu: None,
                }),
                min_size: 1,
                max_size: 1,
            })
            .build();
        cluster.template_inputs = Some(alien_core::TemplateInputs {
            horizond_download_base_url: "http://releases.test".to_string(),
            horizon_api_url: "http://horizon.test".to_string(),
            horizond_binary_hash: None,
            monitoring_logs_endpoint: None,
            monitoring_metrics_endpoint: None,
            monitoring_auth_hash: None,
            monitoring_metrics_auth_hash: None,
        });
        cluster
    }

    fn test_network() -> Network {
        Network::new("default-network".to_string())
            .settings(NetworkSettings::Create {
                cidr: Some("10.0.0.0/16".to_string()),
                availability_zones: 2,
            })
            .build()
    }

    fn test_env_vars() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: vec![EnvironmentVariable {
                name: "ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE".to_string(),
                value: "hj_test".to_string(),
                var_type: EnvironmentVariableType::Secret,
                target_resources: None,
            }],
            hash: String::new(),
            created_at: String::new(),
        }
    }

    #[tokio::test]
    async fn test_create_and_delete_flow_succeeds() {
        let cluster_id = "test-cluster";

        // stamp_template_inputs stamps horizon_api_url from HorizonConfig.url,
        // so the compute_backend must use the mock server URL.
        let horizon_server = MockServer::start_async().await;
        horizon_server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!("/clusters/{}", cluster_id));
            then.status(200)
                .json_body(serde_json::json!({"success": true}));
        });

        let (identity, authorization, key_vault, network, compute, lro) =
            mock_azure_clients_for_create_delete();
        let mock_provider =
            setup_mock_provider(identity, authorization, key_vault, network, compute, lro);

        let mut clusters = HashMap::new();
        clusters.insert(
            "compute".to_string(),
            HorizonClusterConfig {
                cluster_id: cluster_id.to_string(),
                management_token: "hm_test".to_string(),
            },
        );
        let compute_backend = ComputeBackend::Horizon(HorizonConfig {
            url: horizon_server.base_url(),
            horizond_download_base_url: "http://releases.test".to_string(),
            horizond_binary_hash: None,
            clusters,
        });

        let mut cluster = test_cluster();
        cluster.template_inputs.as_mut().unwrap().horizon_api_url = horizon_server.base_url();

        let mut executor = SingleControllerExecutor::builder()
            .resource(cluster)
            .controller(AzureContainerClusterController::default())
            .platform(alien_core::Platform::Azure)
            .compute_backend(compute_backend)
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
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

        let horizon_server = MockServer::start_async().await;
        horizon_server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!("/clusters/{}", cluster_id));
            then.status(200)
                .json_body(serde_json::json!({"success": true}));
        });

        let (identity, authorization, key_vault, network, compute, lro) =
            mock_azure_clients_for_create_delete();
        let mock_provider =
            setup_mock_provider(identity, authorization, key_vault, network, compute, lro);

        let mut ready_controller =
            AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);
        ready_controller.horizon_api_url = Some(horizon_server.base_url());

        let mut original_cluster = test_cluster();
        original_cluster
            .template_inputs
            .as_mut()
            .unwrap()
            .horizon_api_url = horizon_server.base_url();
        let mut updated_cluster = original_cluster.clone();
        updated_cluster.capacity_groups[0].min_size = 2;

        let mut executor = SingleControllerExecutor::builder()
            .resource(original_cluster)
            .controller(ready_controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.update(updated_cluster).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }

    #[tokio::test]
    async fn test_best_effort_deletion_when_resources_missing() {
        let cluster_id = "test-cluster";
        let (identity, authorization, key_vault, network, compute, lro) =
            mock_azure_clients_for_best_effort_delete();
        let mock_provider =
            setup_mock_provider(identity, authorization, key_vault, network, compute, lro);

        let ready_controller =
            AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_cluster())
            .controller(ready_controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        executor.delete().unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Deleted);
    }

    #[tokio::test]
    async fn test_rolling_update_on_template_inputs_change() {
        let cluster_id = "test-cluster";

        let horizon_server = MockServer::start_async().await;
        horizon_server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!("/clusters/{}", cluster_id));
            then.status(200)
                .json_body(serde_json::json!({"success": true}));
        });

        // Build mocks that support rolling update: VMSS model update + rolling upgrade
        let mut compute = MockVirtualMachineScaleSetsApi::new();
        let (identity, authorization, key_vault, network, _, lro) =
            mock_azure_clients_for_create_delete();

        // Structural update: get + update VMSS for resize
        compute.expect_get_vmss().returning(|_, _| Ok(test_vmss()));
        compute
            .expect_create_or_update_vmss()
            .returning(|_, _, _| Ok(OperationResult::Completed(test_vmss())));
        // Rolling update: start rolling upgrade returns an LRO
        compute
            .expect_start_vmss_rolling_upgrade()
            .returning(|_, _| Ok(OperationResult::Completed(())));
        // Rolling update: poll status returns Completed
        compute
            .expect_get_vmss_rolling_upgrade_latest()
            .returning(|_, _| {
                Ok(RollingUpgradeLatestStatus {
                    properties: Some(RollingUpgradeStatusProperties {
                        running_status: Some(RollingUpgradeRunningStatus {
                            code: Some("Completed".to_string()),
                            start_time: None,
                            last_action: None,
                            last_action_time: None,
                        }),
                        progress: None,
                        error: None,
                    }),
                })
            });
        // Deletion mocks (needed by mock provider setup for delete_vmss)
        compute
            .expect_delete_vmss()
            .returning(|_, _| Ok(OperationResult::Completed(())));
        // List VMs for boot check
        compute.expect_list_vmss_vms().returning(|_, _| {
            Ok(alien_azure_clients::azure::models::compute_rp::VirtualMachineScaleSetVmListResult {
                value: vec![], next_link: None,
            })
        });

        let mock_provider = setup_mock_provider(
            identity,
            authorization,
            key_vault,
            network,
            Arc::new(compute),
            lro,
        );

        let mut ready_controller =
            AzureContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);
        ready_controller.horizon_api_url = Some(horizon_server.base_url());

        // Original config
        let mut original_cluster = test_cluster();
        original_cluster
            .template_inputs
            .as_mut()
            .unwrap()
            .horizon_api_url = horizon_server.base_url();

        // Updated config: DIFFERENT template_inputs to trigger rolling update
        let mut updated_cluster = original_cluster.clone();
        updated_cluster
            .template_inputs
            .as_mut()
            .unwrap()
            .horizond_download_base_url = "http://new-releases.test".to_string();

        let mut executor = SingleControllerExecutor::builder()
            .resource(original_cluster)
            .controller(ready_controller)
            .platform(alien_core::Platform::Azure)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                AzureNetworkController::mock_ready("default-network"),
            )
            .with_test_dependencies()
            .build()
            .await
            .unwrap();

        assert_eq!(executor.status(), ResourceStatus::Running);

        executor.update(updated_cluster).unwrap();
        executor.run_until_terminal().await.unwrap();
        assert_eq!(executor.status(), ResourceStatus::Running);
    }
}
