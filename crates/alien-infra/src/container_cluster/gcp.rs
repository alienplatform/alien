//! GCP ContainerCluster Controller
//!
//! This module implements the GCP-specific controller for managing ContainerCluster resources.
//! A ContainerCluster provisions the compute infrastructure for running containers via Horizon:
//!
//! - Service Account for VM instances (to access Secret Manager, call GCP APIs, etc.)
//! - Secret Manager secret for horizond machine token
//! - Instance Template with horizond agent configuration
//! - Firewall rules for instance communication
//! - Managed Instance Groups (one per capacity group)
//!
//! The platform creates the Horizon cluster via the Horizon API before deployment.
//! This controller provisions the cloud infrastructure that machines use to join the cluster.

use alien_gcp_clients::gcp::compute::{
    AccessConfig, AccessConfigType, ComputeApi, Firewall, FirewallAllowed, FixedOrPercent,
    HealthCheck, HealthCheckType, HttpHealthCheck, InstanceGroupManager,
    InstanceGroupManagerAutoHealingPolicy, InstanceGroupManagerUpdatePolicy, InstanceProperties,
    InstanceTemplate, ManagedInstanceCurrentAction, ManagedInstanceStatus, Metadata, MetadataItem,
    MinimalAction, NetworkInterface, ServiceAccount as GcpServiceAccount, Tags, UpdatePolicyType,
};
// BOOT_DIAG_TIMEOUT_ITERATIONS: number of 10-second polling iterations (~5 minutes).
// After this many iterations without all MIGs reaching Running state we fetch serial
// port output from one instance and surface it in the error for debugging.
const BOOT_DIAG_TIMEOUT_ITERATIONS: u32 = 30;
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::{
    CapacityGroup, CapacityGroupStatus, ComputeBackend, ContainerCluster, ContainerClusterOutputs,
    Network, NetworkSettings, ResourceOutputs, ResourceRef, ResourceStatus, TemplateInputs,
};
use alien_error::{AlienError, Context};
use alien_gcp_clients::gcp::iam::{Binding, CreateServiceAccountRequest, ServiceAccount};
use alien_gcp_clients::gcp::secret_manager::{
    AddSecretVersionRequest, AutomaticReplication, Replication, ReplicationPolicy, Secret,
    SecretManagerApi, SecretPayload,
};
use alien_macros::controller;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, to_horizon_capacity_groups};
use crate::network::GcpNetworkController;

/// Tracks the state of a single instance template (one per capacity group).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTemplateState {
    /// Capacity group ID this template is for
    pub group_id: String,
    /// Template name in GCP
    pub template_name: String,
    /// Template self link URL
    pub self_link: String,
}

/// Tracks the state of a single Managed Instance Group (one per capacity group).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MigState {
    /// Capacity group ID this MIG is for
    pub group_id: String,
    /// MIG name
    pub mig_name: Option<String>,
    /// Zone where MIG is deployed
    pub zone: Option<String>,
    /// Current number of instances
    pub current_size: u32,
    /// Desired number of instances (from capacity plan)
    pub desired_size: u32,
    /// Instance type used
    pub instance_type: Option<String>,
}

/// GCP ContainerCluster Controller state machine.
///
/// This controller manages the lifecycle of GCP infrastructure for container workloads:
/// - Service Account for VM instances
/// - Secret Manager secret for machine token storage
/// - Instance Template with horizond configuration
/// - Firewall rules for cluster networking
/// - Managed Instance Groups (one per capacity group)
#[controller]
pub struct GcpContainerClusterController {
    // Service Account
    pub(crate) service_account_email: Option<String>,
    pub(crate) service_account_name: Option<String>,

    // Secret Manager (for machine token and OTLP auth headers)
    pub(crate) machine_token_secret_name: Option<String>,
    /// Secret Manager secret ID for the OTLP logs auth header (optional).
    #[serde(default)]
    pub(crate) otlp_auth_secret_name: Option<String>,
    /// Secret Manager secret ID for the OTLP metrics auth header (optional).
    /// Only set when metrics uses a separate auth header from logs (e.g. different Axiom dataset).
    #[serde(default)]
    pub(crate) otlp_metrics_auth_secret_name: Option<String>,

    // Instance Templates (one per capacity group)
    pub(crate) instance_templates: HashMap<String, InstanceTemplateState>,

    // Firewall Rule
    pub(crate) firewall_rule_name: Option<String>,
    /// Separate firewall rule allowing GCP health checker IPs to reach port 8080.
    /// GCP health checks come from 35.191.0.0/16 and 130.211.0.0/22, which are outside
    /// the VPC CIDR, so they need their own rule.
    #[serde(default)]
    pub(crate) health_check_firewall_rule_name: Option<String>,

    // Health Check (for MIG auto-healing)
    #[serde(default)]
    pub(crate) health_check_name: Option<String>,
    #[serde(default)]
    pub(crate) health_check_self_link: Option<String>,

    // Managed Instance Groups (one per capacity group)
    pub(crate) mig_states: HashMap<String, MigState>,

    // Horizon cluster info
    pub(crate) horizon_cluster_id: Option<String>,
    pub(crate) horizon_ready: bool,
    pub(crate) horizon_api_url: Option<String>,

    // Boot diagnostics: counts iterations spent waiting for MIGs to become ready.
    // When the threshold is reached we fetch serial port output before failing.
    #[serde(default)]
    pub(crate) boot_check_iterations: u32,

    /// Groups newly created during an update flow, waiting for their machines to boot.
    #[serde(default)]
    pub(crate) new_groups_pending_ready: Vec<String>,

    /// Counts iterations spent waiting for rolling updates to complete.
    #[serde(default)]
    pub(crate) rolling_update_poll_iterations: u32,

    /// Old instance template names to delete after a rolling update completes.
    /// Maps group_id -> old template name. GCP templates are immutable, so we
    /// create a new versioned template and clean up the old one after rollout.
    #[serde(default)]
    pub(crate) old_instance_templates_to_delete: HashMap<String, String>,

    /// Whether a rolling update was triggered in the current update cycle.
    #[serde(default)]
    pub(crate) rolling_update_triggered: bool,
}

impl GcpContainerClusterController {
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

    /// Create or update a single Secret Manager secret with an OTLP auth header value,
    /// granting the given service account secretAccessor access.
    async fn gcp_upsert_otlp_secret(
        &self,
        client: &dyn SecretManagerApi,
        secret_id: &str,
        value: &str,
        sa_email: &str,
        config: &ContainerCluster,
    ) -> Result<()> {
        match client.get_secret(secret_id.to_string()).await {
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                client
                    .create_secret(
                        secret_id.to_string(),
                        Secret::builder()
                            .replication(Replication {
                                replication_policy: Some(ReplicationPolicy::Automatic(
                                    AutomaticReplication::default(),
                                )),
                            })
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create OTLP secret '{}'", secret_id),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
            Err(e) => {
                return Err(e).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to check OTLP secret '{}' existence", secret_id),
                    resource_id: Some(config.id.clone()),
                });
            }
        }

        client
            .add_secret_version(
                secret_id.to_string(),
                AddSecretVersionRequest {
                    payload: SecretPayload {
                        data: Some(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            value.as_bytes(),
                        )),
                    },
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to add version for OTLP secret '{}'", secret_id),
                resource_id: Some(config.id.clone()),
            })?;

        let mut policy = client
            .get_secret_iam_policy(secret_id.to_string())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for OTLP secret '{}'", secret_id),
                resource_id: Some(config.id.clone()),
            })?;

        let member = format!("serviceAccount:{}", sa_email);
        if !policy
            .bindings
            .iter()
            .any(|b| b.role == "roles/secretmanager.secretAccessor" && b.members.contains(&member))
        {
            policy.bindings.push(Binding {
                role: "roles/secretmanager.secretAccessor".to_string(),
                members: vec![member],
                condition: None,
            });
            client
                .set_secret_iam_policy(secret_id.to_string(), policy)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to set IAM policy for OTLP secret '{}'", secret_id),
                    resource_id: Some(config.id.clone()),
                })?;
        }

        Ok(())
    }

    /// Generate startup script for horizond.
    fn generate_startup_script(
        &self,
        cluster_id: &str,
        api_url: &str,
        horizond_download_base_url: &str,
        project_id: &str,
        secret_name: &str,
        capacity_group_prefix: &str,
        otlp_logs_endpoint: Option<&str>,
        otlp_metrics_endpoint: Option<&str>,
        otlp_auth_secret_name: Option<&str>,
        otlp_metrics_auth_secret_name: Option<&str>,
    ) -> String {
        // Bash script that:
        // 1. Fetches machine token (and optionally OTLP auth headers) from Secret Manager
        // 2. Installs horizond dependencies
        // 3. Starts horizond with cluster configuration
        let horizond_url =
            super::join_url_path(horizond_download_base_url, "linux-x86_64/horizond");

        // Build the optional block that fetches the OTLP logs auth header from Secret Manager.
        let otlp_secret_fetch = match otlp_auth_secret_name {
            Some(sn) => format!(
                r#"OTLP_SECRET_PATH="projects/{}/secrets/{}/versions/latest:access"
OTLP_AUTH_HEADER=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://secretmanager.googleapis.com/v1/$OTLP_SECRET_PATH" | jq -r .payload.data | base64 -d)"#,
                project_id, sn
            ),
            None => String::new(),
        };

        // Fetch the OTLP metrics auth header from a separate secret if configured.
        // When absent, fall back to the same header as logs.
        let otlp_metrics_secret_fetch = match otlp_metrics_auth_secret_name {
            Some(sn) => format!(
                r#"
OTLP_METRICS_SECRET_PATH="projects/{}/secrets/{}/versions/latest:access"
OTLP_METRICS_AUTH_HEADER=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://secretmanager.googleapis.com/v1/$OTLP_METRICS_SECRET_PATH" | jq -r .payload.data | base64 -d)"#,
                project_id, sn
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

            if let Some(ep) = otlp_logs_endpoint {
                flags.push(format!("  --otlp-logs-endpoint \"{}\" \\", ep));
                if has_logs_auth {
                    flags.push("  --otlp-logs-auth-header \"$OTLP_AUTH_HEADER\" \\".to_string());
                }
            }
            if let Some(ep) = otlp_metrics_endpoint {
                flags.push(format!("  --otlp-metrics-endpoint \"{}\" \\", ep));
                if has_logs_auth {
                    flags.push(format!(
                        "  --otlp-metrics-auth-header \"{}\" \\",
                        metrics_auth_var
                    ));
                }
            }
            flags.join("\n")
        };

        format!(
            r#"#!/bin/bash
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

cat > /etc/sysctl.d/99-horizon.conf << 'SYSCTL_EOF'
net.ipv4.ip_forward=1
net.ipv6.conf.all.disable_ipv6=1
net.ipv6.conf.default.disable_ipv6=1
SYSCTL_EOF
sysctl -p /etc/sysctl.d/99-horizon.conf > /dev/null

INSTANCE_NAME=$(curl -s -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/name)
ZONE=$(curl -s -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/zone | cut -d/ -f4)
CREATED_BY=$(curl -s -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/attributes/created-by || true)
MIG_NAME=$(basename "$CREATED_BY")
CAPACITY_GROUP="${{MIG_NAME#{}}}"
if [ -z "$CAPACITY_GROUP" ] || [ "$CAPACITY_GROUP" = "$MIG_NAME" ]; then
  CAPACITY_GROUP="general"
fi

ACCESS_TOKEN=$(curl -s -H "Metadata-Flavor: Google" \
  http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token | jq -r .access_token)
SECRET_PATH="projects/{}/secrets/{}/versions/latest:access"
MACHINE_TOKEN=$(curl -s -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://secretmanager.googleapis.com/v1/$SECRET_PATH" | jq -r .payload.data | base64 -d)
{}

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
  --machine-id $INSTANCE_NAME \\
  --machine-token $MACHINE_TOKEN \\
  --api-url "{}" \\
  --zone $ZONE \\
  --network-interface ens4 \\
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

systemctl daemon-reload
systemctl enable horizond
log "horizond_starting"
systemctl start horizond
"#,
            capacity_group_prefix,
            project_id,
            secret_name,
            format!("{}{}", otlp_secret_fetch, otlp_metrics_secret_fetch),
            horizond_url,
            cluster_id,
            api_url,
            if otlp_flags.is_empty() {
                String::new()
            } else {
                // Prepend ` \` so the preceding --capacity-group line continues into the
                // OTLP flags. Remove the trailing ` \` from the last flag so the following
                // Restart= directive is not consumed as a line continuation by systemd.
                let trimmed = otlp_flags.trim_end_matches(" \\").trim_end_matches('\\');
                format!(" \\\n{}", trimmed)
            }
        )
    }

    /// Creates an instance template for a single capacity group.
    /// Used by the Create flow, structural Update flow (adding groups), and rolling Update flow.
    /// When `template_name_override` is provided, uses that name instead of the default
    /// `{prefix}-{id}-{group_id}-template`. This is needed for rolling updates since
    /// GCP instance templates are immutable and require versioned names.
    async fn create_instance_template_for_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group: &CapacityGroup,
        config: &ContainerCluster,
        template_name_override: Option<&str>,
    ) -> Result<()> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        let sa_email = self.service_account_email.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Service account email not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let secret_name = self.machine_token_secret_name.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Machine token secret name not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
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
        let template_inputs: TemplateInputs = config.template_inputs.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "ContainerCluster is missing template_inputs".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let otlp_auth_secret_name = self.otlp_auth_secret_name.clone();

        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<GcpNetworkController>(&network_ref)?;
        let network_url = network.network_self_link.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network self link not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let subnet_url = network.subnetwork_self_link.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No subnetwork available from Network".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let network_settings = network.desired_settings.clone();
        let machine_type = group.instance_type.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Capacity group '{}': instance_type not set", group.group_id),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let group_template_name = match template_name_override {
            Some(name) => name.to_string(),
            None => format!(
                "{}-{}-{}-template",
                ctx.resource_prefix, config.id, group.group_id
            ),
        };
        let capacity_group_prefix = format!("{}-{}-", ctx.resource_prefix, config.id);
        let startup_script = self.generate_startup_script(
            &cluster_id,
            &api_url,
            &template_inputs.horizond_download_base_url,
            &gcp_cfg.project_id,
            &secret_name,
            &capacity_group_prefix,
            template_inputs.monitoring_logs_endpoint.as_deref(),
            template_inputs.monitoring_metrics_endpoint.as_deref(),
            otlp_auth_secret_name.as_deref(),
            self.otlp_metrics_auth_secret_name.as_deref(),
        );

        info!(template_name = %group_template_name, group_id = %group.group_id, machine_type = %machine_type, "Creating instance template");

        let template = InstanceTemplate::builder()
            .name(group_template_name.clone())
            .description(format!(
                "Instance template for ContainerCluster {} capacity group {}",
                config.id, group.group_id
            ))
            .properties(
                InstanceProperties::builder()
                    .machine_type(machine_type.clone())
                    .disks(vec![alien_gcp_clients::gcp::compute::AttachedDisk {
                        r#type: Some(alien_gcp_clients::gcp::compute::AttachedDiskType::Persistent),
                        mode: Some(alien_gcp_clients::gcp::compute::DiskMode::ReadWrite),
                        source: None,
                        device_name: None,
                        boot: Some(true),
                        initialize_params: Some(
                            alien_gcp_clients::gcp::compute::AttachedDiskInitializeParams {
                                source_image: Some(
                                    "projects/ubuntu-os-cloud/global/images/family/ubuntu-2204-lts"
                                        .to_string(),
                                ),
                                ..Default::default()
                            },
                        ),
                        auto_delete: Some(true),
                        index: None,
                        interface: None,
                    }])
                    .network_interfaces({
                        let access_configs =
                            if matches!(&network_settings, Some(NetworkSettings::UseDefault)) {
                                vec![AccessConfig {
                                    r#type: Some(AccessConfigType::OneToOneNat),
                                    name: Some("External NAT".to_string()),
                                    nat_i_p: None,
                                    network_tier: None,
                                }]
                            } else {
                                vec![]
                            };
                        vec![NetworkInterface::builder()
                            .network(network_url.clone())
                            .subnetwork(subnet_url.clone())
                            .access_configs(access_configs)
                            .build()]
                    })
                    .service_accounts(vec![GcpServiceAccount {
                        email: Some(sa_email.clone()),
                        scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
                    }])
                    .metadata(Metadata {
                        items: vec![MetadataItem {
                            key: Some("startup-script".to_string()),
                            value: Some(startup_script),
                        }],
                        fingerprint: None,
                        kind: Some("compute#metadata".to_string()),
                    })
                    .tags(Tags {
                        items: vec!["horizon-container-cluster".to_string()],
                        fingerprint: None,
                    })
                    .build(),
            )
            .build();

        compute_client
            .insert_instance_template(template)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create instance template for group '{}'",
                    group.group_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let self_link = format!(
            "https://compute.googleapis.com/compute/v1/projects/{}/global/instanceTemplates/{}",
            gcp_cfg.project_id, group_template_name
        );

        self.instance_templates.insert(
            group.group_id.clone(),
            InstanceTemplateState {
                group_id: group.group_id.clone(),
                template_name: group_template_name,
                self_link,
            },
        );
        Ok(())
    }

    /// Creates a Managed Instance Group for a single capacity group.
    async fn create_mig_for_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group: &CapacityGroup,
        config: &ContainerCluster,
    ) -> Result<()> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let zone = format!("{}-a", gcp_cfg.region);
        let mig_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);

        let template_state = self
            .instance_templates
            .get(&group.group_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!("No instance template for group '{}'", group.group_id),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        let template_self_link = template_state.self_link.clone();
        let instance_type = group.instance_type.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Capacity group '{}': instance_type not set", group.group_id),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(mig_name = %mig_name, group_id = %group.group_id, min_size = group.min_size, "Creating MIG");

        let auto_healing_policies = self
            .health_check_self_link
            .as_ref()
            .map(|hc_link| {
                vec![InstanceGroupManagerAutoHealingPolicy {
                    health_check: Some(hc_link.clone()),
                    initial_delay_sec: Some(300),
                }]
            })
            .unwrap_or_default();

        let mig = InstanceGroupManager::builder()
            .name(mig_name.clone())
            .description(format!(
                "MIG for ContainerCluster {} capacity group {}",
                config.id, group.group_id
            ))
            .base_instance_name(format!("{}-{}", ctx.resource_prefix, group.group_id))
            .instance_template(template_self_link)
            .target_size(group.min_size as i32)
            .auto_healing_policies(auto_healing_policies)
            .build();

        compute_client
            .insert_instance_group_manager(zone.clone(), mig)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create MIG for group '{}'", group.group_id),
                resource_id: Some(config.id.clone()),
            })?;

        self.mig_states.insert(
            group.group_id.clone(),
            MigState {
                group_id: group.group_id.clone(),
                mig_name: Some(mig_name),
                zone: Some(zone),
                current_size: 0,
                desired_size: group.min_size,
                instance_type: Some(instance_type),
            },
        );
        Ok(())
    }

    /// Deletes the MIG and instance template for a capacity group (best-effort).
    async fn delete_capacity_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group_id: &str,
        resource_id: &str,
    ) -> Result<()> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(state) = self.mig_states.remove(group_id) {
            if let (Some(mig_name), Some(zone)) = (state.mig_name, state.zone) {
                match compute_client
                    .delete_instance_group_manager(zone, mig_name.clone())
                    .await
                {
                    Ok(_) | Err(_) => {} // best-effort
                }
                info!(mig_name = %mig_name, "Deleted MIG for removed capacity group");
            }
        }
        if let Some(tmpl) = self.instance_templates.remove(group_id) {
            match compute_client
                .delete_instance_template(tmpl.template_name.clone())
                .await
            {
                Ok(_) | Err(_) => {} // best-effort
            }
            info!(template_name = %tmpl.template_name, "Deleted instance template for removed capacity group");
        }
        let _ = resource_id;
        Ok(())
    }

    /// Tries to fetch serial port output from one MIG instance for boot diagnostics.
    pub(crate) async fn collect_serial_port_output(
        compute_client: &dyn ComputeApi,
        mig_states: &HashMap<String, MigState>,
    ) -> Option<String> {
        for state in mig_states.values() {
            if let (Some(mig_name), Some(zone)) = (&state.mig_name, &state.zone) {
                let managed_instances = compute_client
                    .list_managed_instances(zone.clone(), mig_name.clone())
                    .await
                    .ok()?;

                for mi in &managed_instances.managed_instances {
                    // Instance URL looks like: .../zones/{zone}/instances/{name}
                    let instance_name = mi.instance.as_ref()?.split('/').last()?;
                    let output = compute_client
                        .get_serial_port_output(zone.clone(), instance_name.to_string())
                        .await
                        .ok()?;
                    if let Some(contents) = output.contents {
                        if !contents.is_empty() {
                            return Some(contents);
                        }
                    }
                }
            }
        }
        None
    }
}

#[controller]
impl GcpContainerClusterController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(cluster_id = %config.id, "Starting GCP ContainerCluster provisioning");

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
            state: CreatingServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingServiceAccount,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let account_id = format!("{}-{}-sa", ctx.resource_prefix, config.id);
        let sa_email = format!(
            "{}@{}.iam.gserviceaccount.com",
            account_id, gcp_cfg.project_id
        );

        info!(account_id = %account_id, "Creating service account for container instances");

        let sa = match iam_client.get_service_account(sa_email.clone()).await {
            Ok(existing) => {
                info!("Service account already exists, reusing");
                existing
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                // Create new service account
                iam_client
                    .create_service_account(
                        account_id.clone(),
                        CreateServiceAccountRequest::builder()
                            .service_account(
                                ServiceAccount::builder()
                                    .display_name(format!(
                                        "Alien ContainerCluster {} SA",
                                        config.id
                                    ))
                                    .description(format!(
                                        "Service account for Alien ContainerCluster {} instances",
                                        config.id
                                    ))
                                    .build(),
                            )
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create service account".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?
            }
            Err(e) => {
                return Err(e).context(ErrorData::CloudPlatformError {
                    message: "Failed to check service account existence".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            }
        };

        self.service_account_email = sa.email.clone();
        self.service_account_name = sa.name.clone();

        info!(
            email = ?sa.email,
            "Service account created/verified"
        );

        Ok(HandlerAction::Continue {
            state: GrantingServiceAccountRoles,
            suggested_delay: Some(Duration::from_secs(10)), // Wait for IAM propagation
        })
    }

    #[handler(
        state = GrantingServiceAccountRoles,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn granting_service_account_roles(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let resource_manager_client = ctx
            .service_provider
            .get_gcp_resource_manager_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let sa_email = self.service_account_email.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Service account email not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        info!(
            service_account = %sa_email,
            "Granting IAM roles to service account"
        );

        let mut policy = resource_manager_client
            .get_project_iam_policy(
                gcp_cfg.project_id.clone(),
                Some(alien_gcp_clients::resource_manager::GetPolicyOptions {
                    requested_policy_version: Some(3),
                }),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to get project IAM policy".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let member = format!("serviceAccount:{}", sa_email);
        let required_roles = [
            "roles/compute.instanceAdmin.v1",
            "roles/compute.networkAdmin",
            "roles/secretmanager.secretAccessor",
            "roles/logging.logWriter",
            // Allows the VM to generate access tokens for per-container service accounts,
            // which is required by the horizond IMDS metadata proxy to vend per-container credentials.
            "roles/iam.serviceAccountTokenCreator",
        ];

        let mut changed = false;
        for role in required_roles {
            if let Some(binding) = policy.bindings.iter_mut().find(|b| b.role == role) {
                if !binding.members.contains(&member) {
                    binding.members.push(member.clone());
                    changed = true;
                }
            } else {
                policy.bindings.push(Binding {
                    role: role.to_string(),
                    members: vec![member.clone()],
                    condition: None,
                });
                changed = true;
            }
        }

        if changed {
            resource_manager_client
                .set_project_iam_policy(gcp_cfg.project_id.clone(), policy, None)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to update project IAM policy".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;
        }

        info!("Service account roles granted");

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
        let gcp_cfg = ctx.get_gcp_config()?;
        let secret_manager_client = ctx
            .service_provider
            .get_gcp_secret_manager_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let token_key = Self::machine_token_key(&config.id);
        let secret_id = format!("{}-secrets-{}", ctx.resource_prefix, token_key);
        let machine_token = Self::machine_token_from_env(ctx, &config.id)?;

        info!(secret_id = %secret_id, "Creating Secret Manager secret for machine token");

        // Ensure secret exists
        match secret_manager_client.get_secret(secret_id.clone()).await {
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                secret_manager_client
                    .create_secret(
                        secret_id.clone(),
                        Secret::builder()
                            .replication(Replication {
                                replication_policy: Some(ReplicationPolicy::Automatic(
                                    AutomaticReplication::default(),
                                )),
                            })
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to create secret".to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
            Err(e) => {
                return Err(e).context(ErrorData::CloudPlatformError {
                    message: "Failed to check secret existence".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            }
        }

        // Add secret version with machine token
        secret_manager_client
            .add_secret_version(
                secret_id.clone(),
                AddSecretVersionRequest {
                    payload: SecretPayload {
                        data: Some(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            machine_token.as_bytes(),
                        )),
                    },
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add secret version".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Grant service account access to the secret
        let sa_email = self.service_account_email.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Service account email not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let mut policy = secret_manager_client
            .get_secret_iam_policy(secret_id.clone())
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to get IAM policy for secret '{}' before granting access. Refusing to proceed to avoid overwriting existing bindings.", secret_id),
                resource_id: Some(config.id.clone()),
            })?;

        // Add binding for service account
        policy.bindings.push(Binding {
            role: "roles/secretmanager.secretAccessor".to_string(),
            members: vec![format!("serviceAccount:{}", sa_email)],
            condition: None,
        });

        secret_manager_client
            .set_secret_iam_policy(secret_id.clone(), policy)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to set secret IAM policy".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.machine_token_secret_name = Some(secret_id);

        info!("Machine token stored in Secret Manager");

        // If OTLP monitoring is configured, store the auth headers as vault secrets.
        // This follows the same pattern as the machine token: the infra controller owns
        // the secret lifecycle, and the startup script fetches them at boot via IAM.
        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let sa_email = self
                .service_account_email
                .as_ref()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Service account email not set".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?
                .clone();

            // Store the logs auth header secret
            let logs_secret_id = format!("{}-secrets-ALIEN_OTLP_AUTH_HEADER", ctx.resource_prefix);
            self.gcp_upsert_otlp_secret(
                secret_manager_client.as_ref(),
                &logs_secret_id,
                &monitoring.logs_auth_header,
                &sa_email,
                config,
            )
            .await?;
            self.otlp_auth_secret_name = Some(logs_secret_id);

            // Store the metrics auth header secret if it differs from logs
            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_id = format!(
                    "{}-secrets-ALIEN_OTLP_METRICS_AUTH_HEADER",
                    ctx.resource_prefix
                );
                self.gcp_upsert_otlp_secret(
                    secret_manager_client.as_ref(),
                    &metrics_secret_id,
                    metrics_auth_header,
                    &sa_email,
                    config,
                )
                .await?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_id);
            }

            info!("OTLP auth headers stored in Secret Manager");
        }

        Ok(HandlerAction::Continue {
            state: CreatingInstanceTemplate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingInstanceTemplate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_instance_template(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let sa_email = self.service_account_email.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Service account email not set".to_string(),
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

        // Get network from dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<GcpNetworkController>(&network_ref)?;
        let network_url = network.network_self_link.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network self link not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let subnet_url = network.subnetwork_self_link.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No subnetwork available from Network".to_string(),
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
        let startup_script = self.generate_startup_script(
            cluster_id,
            api_url,
            &template_inputs.horizond_download_base_url,
            &gcp_cfg.project_id,
            secret_name,
            &capacity_group_prefix,
            template_inputs.monitoring_logs_endpoint.as_deref(),
            template_inputs.monitoring_metrics_endpoint.as_deref(),
            self.otlp_auth_secret_name.as_deref(),
            self.otlp_metrics_auth_secret_name.as_deref(),
        );

        // Create one instance template per capacity group. Each group may have a
        // different instance type (e.g., general = n2-standard-8, gpu = a2-highgpu-1g).
        // GCP instance templates are immutable and have a single machine_type, so
        // per-group templates are required.
        for group in &config.capacity_groups {
            let group_template_name = format!(
                "{}-{}-{}-template",
                ctx.resource_prefix, config.id, group.group_id
            );

            let machine_type = group.instance_type.as_ref().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Capacity group '{}': instance_type not set (should be resolved by preflights)",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(
                template_name = %group_template_name,
                group_id = %group.group_id,
                machine_type = %machine_type,
                "Creating instance template for capacity group"
            );

            let template = InstanceTemplate::builder()
                .name(group_template_name.clone())
                .description(format!(
                    "Instance template for ContainerCluster {} capacity group {}",
                    config.id, group.group_id
                ))
                .properties(
                    InstanceProperties::builder()
                        .machine_type(machine_type.clone())
                        .disks(vec![alien_gcp_clients::gcp::compute::AttachedDisk {
                            r#type: Some(alien_gcp_clients::gcp::compute::AttachedDiskType::Persistent),
                            mode: Some(alien_gcp_clients::gcp::compute::DiskMode::ReadWrite),
                            source: None,
                            device_name: None,
                            boot: Some(true),
                            initialize_params: Some(
                                alien_gcp_clients::gcp::compute::AttachedDiskInitializeParams {
                                    source_image: Some(
                                        "projects/ubuntu-os-cloud/global/images/family/ubuntu-2204-lts"
                                            .to_string(),
                                    ),
                                    ..Default::default()
                                },
                            ),
                            auto_delete: Some(true),
                            index: None,
                            interface: None,
                        }])
                        .network_interfaces({
                            // UseDefault: assign ephemeral public IPs for dev/test egress (no NAT).
                            // Create: NAT handles egress; VMs stay private.
                            // BYO: customer manages egress; no public IPs from Alien.
                            let access_configs =
                                if matches!(&network.desired_settings, Some(NetworkSettings::UseDefault)) {
                                    vec![AccessConfig {
                                        r#type: Some(AccessConfigType::OneToOneNat),
                                        name: Some("External NAT".to_string()),
                                        nat_i_p: None,
                                        network_tier: None,
                                    }]
                                } else {
                                    vec![]
                                };
                            vec![NetworkInterface::builder()
                                .network(network_url.clone())
                                .subnetwork(subnet_url.clone())
                                .access_configs(access_configs)
                                .build()]
                        })
                        .service_accounts(vec![GcpServiceAccount {
                            email: Some(sa_email.clone()),
                            scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
                        }])
                        .metadata(Metadata {
                            items: vec![MetadataItem {
                                key: Some("startup-script".to_string()),
                                value: Some(startup_script.clone()),
                            }],
                            fingerprint: None,
                            kind: Some("compute#metadata".to_string()),
                        })
                        .tags(Tags {
                            items: vec!["horizon-container-cluster".to_string()],
                            fingerprint: None,
                        })
                        .build(),
                )
                .build();

            compute_client
                .insert_instance_template(template)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "Failed to create instance template for capacity group '{}'",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })?;

            let self_link = format!(
                "https://compute.googleapis.com/compute/v1/projects/{}/global/instanceTemplates/{}",
                gcp_cfg.project_id, group_template_name
            );

            info!(
                template_name = %group_template_name,
                self_link = %self_link,
                group_id = %group.group_id,
                "Instance template created for capacity group"
            );

            self.instance_templates.insert(
                group.group_id.clone(),
                InstanceTemplateState {
                    group_id: group.group_id.clone(),
                    template_name: group_template_name,
                    self_link,
                },
            );
        }

        info!(
            template_count = self.instance_templates.len(),
            "All instance templates created"
        );

        Ok(HandlerAction::Continue {
            state: CreatingFirewallRules,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingFirewallRules,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_firewall_rules(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let firewall_name = format!("{}-{}-fw", ctx.resource_prefix, config.id);

        // Get network from dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<GcpNetworkController>(&network_ref)?;
        let network_url = network.network_self_link.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network self link not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let cidr_block = network.cidr_block.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network CIDR block not available. Cannot create firewall rules without a known CIDR range — refusing to default to 0.0.0.0/0 which would open the firewall to the entire internet.".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let source_ranges = vec![cidr_block.clone()];

        info!(firewall_name = %firewall_name, "Creating firewall rules");

        let firewall = Firewall::builder()
            .name(firewall_name.clone())
            .description(format!(
                "Firewall rules for Alien ContainerCluster {}",
                config.id
            ))
            .network(network_url.clone())
            .target_tags(vec!["horizon-container-cluster".to_string()])
            .allowed(vec![
                // WireGuard mesh traffic (UDP 51820)
                FirewallAllowed {
                    ip_protocol: Some("udp".to_string()),
                    ports: vec!["51820".to_string()],
                },
                // Container ports (8001-8999)
                FirewallAllowed {
                    ip_protocol: Some("tcp".to_string()),
                    ports: vec!["8001-8999".to_string()],
                },
                // HTTP for load balancers
                FirewallAllowed {
                    ip_protocol: Some("tcp".to_string()),
                    ports: vec!["80".to_string(), "443".to_string()],
                },
                // SSH for management
                FirewallAllowed {
                    ip_protocol: Some("tcp".to_string()),
                    ports: vec!["22".to_string()],
                },
                // All traffic within container network
                FirewallAllowed {
                    ip_protocol: Some("all".to_string()),
                    ports: vec![],
                },
            ])
            .source_ranges(source_ranges)
            .build();

        compute_client
            .insert_firewall(firewall)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create firewall rule".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.firewall_rule_name = Some(firewall_name);

        // GCP health check probes come from 35.191.0.0/16 and 130.211.0.0/22 — Google's
        // infrastructure IPs, outside the VPC CIDR. Without this separate rule they are
        // silently dropped and the health check never passes.
        let hc_firewall_name = format!("{}-{}-hc-fw", ctx.resource_prefix, config.id);
        info!(firewall_name = %hc_firewall_name, "Creating health check firewall rule");

        let hc_firewall = Firewall::builder()
            .name(hc_firewall_name.clone())
            .description(format!(
                "Allow GCP health checker IPs to reach horizond /health (port 8080) for ContainerCluster {}",
                config.id
            ))
            .network(network_url.clone())
            .target_tags(vec!["horizon-container-cluster".to_string()])
            .allowed(vec![FirewallAllowed {
                ip_protocol: Some("tcp".to_string()),
                ports: vec!["8080".to_string()],
            }])
            .source_ranges(vec![
                "35.191.0.0/16".to_string(),
                "130.211.0.0/22".to_string(),
            ])
            .build();

        compute_client.insert_firewall(hc_firewall).await.context(
            ErrorData::CloudPlatformError {
                message: "Failed to create health check firewall rule".to_string(),
                resource_id: Some(config.id.clone()),
            },
        )?;

        self.health_check_firewall_rule_name = Some(hc_firewall_name);

        info!("Firewall rules created");

        Ok(HandlerAction::Continue {
            state: CreatingHealthCheck,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingHealthCheck,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_health_check(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let hc_name = format!("{}-{}-hc", ctx.resource_prefix, config.id);

        info!(health_check_name = %hc_name, "Creating health check for MIG auto-healing");

        let health_check = HealthCheck::builder()
            .name(hc_name.clone())
            .description(format!(
                "Health check for ContainerCluster {} MIG auto-healing",
                config.id
            ))
            .r#type(HealthCheckType::Http)
            .http_health_check(
                HttpHealthCheck::builder()
                    .port(8080)
                    .request_path("/health".to_string())
                    .build(),
            )
            .check_interval_sec(30)
            .timeout_sec(10)
            .healthy_threshold(2)
            .unhealthy_threshold(3)
            .build();

        compute_client
            .insert_health_check(health_check)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create health check".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let self_link = format!(
            "https://compute.googleapis.com/compute/v1/projects/{}/global/healthChecks/{}",
            gcp_cfg.project_id, hc_name
        );

        self.health_check_name = Some(hc_name);
        self.health_check_self_link = Some(self_link);

        info!("Health check created");

        Ok(HandlerAction::Continue {
            state: CreatingManagedInstanceGroups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingManagedInstanceGroups,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_managed_instance_groups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        // TODO: Support multi-zone deployment
        // For now, use a default zone derived from the region.
        let zone = format!("{}-a", gcp_cfg.region);

        info!(
            capacity_groups = config.capacity_groups.len(),
            zone = %zone,
            "Creating Managed Instance Groups for capacity groups"
        );

        for group in &config.capacity_groups {
            let mig_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);

            let template_state = self.instance_templates.get(&group.group_id).ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "No instance template found for capacity group '{}' (CreatingInstanceTemplate should have created it)",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;
            let template_self_link = &template_state.self_link;

            let instance_type = group.instance_type.clone().ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Capacity group '{}': instance_type not set (should be resolved by preflights)",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;

            info!(
                mig_name = %mig_name,
                group_id = %group.group_id,
                instance_type = %instance_type,
                min_size = group.min_size,
                max_size = group.max_size,
                "Creating Managed Instance Group"
            );

            let auto_healing_policies = self
                .health_check_self_link
                .as_ref()
                .map(|hc_link| {
                    vec![InstanceGroupManagerAutoHealingPolicy {
                        health_check: Some(hc_link.clone()),
                        initial_delay_sec: Some(300),
                    }]
                })
                .unwrap_or_default();

            let mig = InstanceGroupManager::builder()
                .name(mig_name.clone())
                .description(format!(
                    "MIG for ContainerCluster {} capacity group {}",
                    config.id, group.group_id
                ))
                .base_instance_name(format!("{}-{}", ctx.resource_prefix, group.group_id))
                .instance_template(template_self_link.clone())
                .target_size(group.min_size as i32)
                .auto_healing_policies(auto_healing_policies)
                .build();

            compute_client
                .insert_instance_group_manager(zone.clone(), mig)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create MIG for capacity group {}", group.group_id),
                    resource_id: Some(config.id.clone()),
                })?;

            self.mig_states.insert(
                group.group_id.clone(),
                MigState {
                    group_id: group.group_id.clone(),
                    mig_name: Some(mig_name),
                    zone: Some(zone.clone()),
                    current_size: 0,
                    desired_size: group.min_size,
                    instance_type: Some(instance_type),
                },
            );
        }

        info!("All Managed Instance Groups created");

        Ok(HandlerAction::Continue {
            state: WaitingForMigsReady,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = WaitingForMigsReady,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_migs_ready(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let mut all_ready = true;
        let mut total_instances = 0u32;
        let mut instance_diagnostics: Vec<String> = Vec::new();

        for state in self.mig_states.values_mut() {
            if let (Some(mig_name), Some(zone)) = (&state.mig_name, &state.zone) {
                let managed_instances = compute_client
                    .list_managed_instances(zone.clone(), mig_name.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to list managed instances for MIG {}", mig_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                // An instance is only truly ready when:
                // - OS is Running (VM is up)
                // - currentAction is None (no ongoing operation)
                //
                // currentAction transitions: CREATING → VERIFYING → NONE
                // VERIFYING means the health check probe is in progress.
                // NONE is only reached once the health check passes.
                // If we moved to Ready while instances are still VERIFYING, auto-healing
                // would soon recreate them and we'd be in a broken steady state.
                let ready_count = managed_instances
                    .managed_instances
                    .iter()
                    .filter(|i| {
                        i.instance_status == Some(ManagedInstanceStatus::Running)
                            && matches!(
                                i.current_action,
                                None | Some(ManagedInstanceCurrentAction::None)
                            )
                    })
                    .count();

                let verifying_count = managed_instances
                    .managed_instances
                    .iter()
                    .filter(|i| {
                        matches!(
                            i.current_action,
                            Some(ManagedInstanceCurrentAction::Verifying)
                        )
                    })
                    .count();

                debug!(
                    mig_name = %mig_name,
                    ready = ready_count,
                    verifying = verifying_count,
                    desired = state.desired_size,
                    "MIG instance status"
                );

                if ready_count < state.desired_size as usize {
                    all_ready = false;
                }

                // Collect per-instance health diagnostics for timeout error messages.
                for inst in &managed_instances.managed_instances {
                    let name = inst
                        .instance
                        .as_deref()
                        .and_then(|url| url.split('/').last())
                        .unwrap_or("unknown");
                    let action = format!(
                        "{:?}",
                        inst.current_action
                            .as_ref()
                            .unwrap_or(&ManagedInstanceCurrentAction::None)
                    );
                    let health = inst
                        .instance_health
                        .first()
                        .and_then(|h| h.detailed_health_state.as_ref())
                        .map(|s| format!("{:?}", s))
                        .unwrap_or_else(|| "no-health-check".to_string());
                    instance_diagnostics
                        .push(format!("  {}: action={} health={}", name, action, health));
                }

                let running_count = ready_count;

                state.current_size = running_count as u32;
                total_instances += running_count as u32;
            }
        }

        if all_ready && total_instances > 0 {
            info!(
                total_instances = total_instances,
                "All MIG instances running"
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
                        Self::collect_serial_port_output(&*compute_client, &self.mig_states).await;
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "No instances appeared after {} iterations (~5 minutes).\n{}",
                            BOOT_DIAG_TIMEOUT_ITERATIONS,
                            super::summarize_boot_log(&boot_log.unwrap_or_default()),
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }

                debug!("Waiting for instances to launch");
                Ok(HandlerAction::Stay {
                    max_times: BOOT_DIAG_TIMEOUT_ITERATIONS + 1,
                    suggested_delay: Some(Duration::from_secs(10)),
                })
            }
        } else {
            self.boot_check_iterations += 1;

            if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
                // Instances are still not ready after the timeout – fetch serial port output
                // from one instance to surface boot errors before failing.
                let boot_log =
                    Self::collect_serial_port_output(&*compute_client, &self.mig_states).await;
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Instances did not become ready after {} iterations (~5 minutes).\nInstance status:\n{}\n{}",
                        BOOT_DIAG_TIMEOUT_ITERATIONS,
                        instance_diagnostics.join("\n"),
                        super::summarize_boot_log(&boot_log.unwrap_or_default()),
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }

            debug!(
                total_instances = total_instances,
                iteration = self.boot_check_iterations,
                "Waiting for more instances to become ready"
            );
            Ok(HandlerAction::Stay {
                max_times: BOOT_DIAG_TIMEOUT_ITERATIONS + 1,
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
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        debug!(cluster_id = %config.id, "GCP ContainerCluster ready, checking health");

        // Periodic health check - verify MIGs exist and update instance counts
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for state in self.mig_states.values_mut() {
            if let (Some(mig_name), Some(zone)) = (&state.mig_name, &state.zone) {
                let managed_instances = compute_client
                    .list_managed_instances(zone.clone(), mig_name.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to list managed instances during health check"),
                        resource_id: Some(config.id.clone()),
                    })?;

                let running_count = managed_instances
                    .managed_instances
                    .iter()
                    .filter(|i| i.instance_status == Some(ManagedInstanceStatus::Running))
                    .count();

                state.current_size = running_count as u32;
            }
        }

        // TODO: Poll Horizon capacity plan API every 60s and adjust MIG sizes
        // let capacity_plan = horizon_client.get_capacity_plan(cluster_id).await?;
        // for group in capacity_plan.groups {
        //     if let Some(state) = self.mig_states.get_mut(&group.group_id) {
        //         if state.current_size != group.desired_machines {
        //             compute_client.resize_instance_group_manager(
        //                 zone, mig_name, group.desired_machines as i32
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
        info!(cluster_id = %config.id, "GCP ContainerCluster update requested");
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
        let gcp_cfg = ctx.get_gcp_config()?;
        let secret_manager_client = ctx
            .service_provider
            .get_gcp_secret_manager_client(gcp_cfg)?;

        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let sa_email = self
                .service_account_email
                .as_ref()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ResourceConfigInvalid {
                        message: "Service account email not set".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?
                .clone();

            // Update (or create) the logs auth header secret
            let logs_secret_id = format!("{}-secrets-ALIEN_OTLP_AUTH_HEADER", ctx.resource_prefix);
            self.gcp_upsert_otlp_secret(
                secret_manager_client.as_ref(),
                &logs_secret_id,
                &monitoring.logs_auth_header,
                &sa_email,
                &config,
            )
            .await?;
            self.otlp_auth_secret_name = Some(logs_secret_id);

            // Update (or create) the metrics auth header secret if present
            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_id = format!(
                    "{}-secrets-ALIEN_OTLP_METRICS_AUTH_HEADER",
                    ctx.resource_prefix
                );
                self.gcp_upsert_otlp_secret(
                    secret_manager_client.as_ref(),
                    &metrics_secret_id,
                    metrics_auth_header,
                    &sa_email,
                    &config,
                )
                .await?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_id);
            } else {
                // Metrics no longer has a separate auth header — clear the stored name
                // (the secret still exists but won't be referenced in the startup script)
                self.otlp_metrics_auth_secret_name = None;
            }

            info!("OTLP secrets updated in Secret Manager");
        } else {
            // Monitoring removed — clear secret references (secrets remain in vault for history)
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
                    message: format!("Failed to sync Horizon cluster capacity groups: {}", e),
                    resource_id: Some(config.id.clone()),
                })
            })?;
        info!(cluster_id = %cluster_id, groups = config.capacity_groups.len(), "Synced Horizon cluster capacity groups");
        self.horizon_ready = true;
        Ok(HandlerAction::Continue {
            state: ResizingExistingMigs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ResizingExistingMigs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn resizing_existing_migs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for group in &config.capacity_groups {
            if let Some(state) = self.mig_states.get_mut(&group.group_id) {
                if let (Some(mig_name), Some(zone)) = (state.mig_name.clone(), state.zone.clone()) {
                    if state.desired_size != group.min_size {
                        let new_min = group.min_size;
                        compute_client
                            .resize_instance_group_manager(zone, mig_name.clone(), new_min as i32)
                            .await
                            .context(ErrorData::CloudPlatformError {
                                message: format!("Failed to resize MIG {}", mig_name),
                                resource_id: Some(config.id.clone()),
                            })?;
                        state.desired_size = new_min;
                        info!(mig_name = %mig_name, new_size = new_min, "Resized MIG");
                    }
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingRemovedMigs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRemovedMigs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn deleting_removed_migs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let removed_group_ids: Vec<String> = self
            .mig_states
            .keys()
            .filter(|id| !config.capacity_groups.iter().any(|g| &g.group_id == *id))
            .cloned()
            .collect();

        for group_id in &removed_group_ids {
            info!(group_id = %group_id, "Removing capacity group");
            self.delete_capacity_group(ctx, group_id, &config.id)
                .await?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingNewGroupTemplates,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNewGroupTemplates,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_group_templates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let new_groups: Vec<CapacityGroup> = config
            .capacity_groups
            .iter()
            .filter(|g| !self.mig_states.contains_key(&g.group_id))
            .cloned()
            .collect();

        for group in &new_groups {
            if self.instance_templates.contains_key(&group.group_id) {
                continue;
            }
            info!(group_id = %group.group_id, "Creating instance template for new capacity group");
            self.create_instance_template_for_group(ctx, group, &config, None)
                .await?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingNewMigs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNewMigs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_migs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let new_groups: Vec<CapacityGroup> = config
            .capacity_groups
            .iter()
            .filter(|g| !self.mig_states.contains_key(&g.group_id))
            .cloned()
            .collect();

        for group in &new_groups {
            info!(group_id = %group.group_id, "Creating MIG for new capacity group");
            self.create_mig_for_group(ctx, group, &config).await?;
        }

        self.new_groups_pending_ready = new_groups.iter().map(|g| g.group_id.clone()).collect();
        Ok(HandlerAction::Continue {
            state: WaitingForNewMigsReady,
            suggested_delay: if self.new_groups_pending_ready.is_empty() {
                None
            } else {
                Some(Duration::from_secs(10))
            },
        })
    }

    #[handler(
        state = WaitingForNewMigsReady,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_new_migs_ready(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.new_groups_pending_ready.is_empty() {
            return Ok(HandlerAction::Continue {
                state: CreatingNewInstanceTemplate,
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
                state: CreatingNewInstanceTemplate,
                suggested_delay: None,
            });
        }

        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
        let mut all_ready = true;
        let mut total_running = 0u32;

        for group_id in &self.new_groups_pending_ready {
            if let Some(state) = self.mig_states.get_mut(group_id) {
                if let (Some(mig_name), Some(zone)) = (&state.mig_name, &state.zone) {
                    let instances = compute_client
                        .list_managed_instances(zone.clone(), mig_name.clone())
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to list instances for MIG {}", mig_name),
                            resource_id: Some(config.id.clone()),
                        })?;
                    let running = instances
                        .managed_instances
                        .iter()
                        .filter(|i| i.instance_status == Some(ManagedInstanceStatus::Running))
                        .count();
                    if running < state.desired_size as usize {
                        all_ready = false;
                    }
                    state.current_size = running as u32;
                    total_running += running as u32;
                }
            }
        }

        if all_ready && total_running > 0 {
            info!(total_running, "All new MIG instances ready");
            self.new_groups_pending_ready.clear();
            return Ok(HandlerAction::Continue {
                state: CreatingNewInstanceTemplate,
                suggested_delay: None,
            });
        }

        self.boot_check_iterations += 1;
        if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
            let new_states: HashMap<String, MigState> = self
                .mig_states
                .iter()
                .filter(|(id, _)| self.new_groups_pending_ready.contains(id))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let boot_log = Self::collect_serial_port_output(&*compute_client, &new_states).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "New MIG instances did not become ready after ~5 minutes.\n{}",
                    super::summarize_boot_log(&boot_log.unwrap_or_default())
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        Ok(HandlerAction::Stay {
            max_times: BOOT_DIAG_TIMEOUT_ITERATIONS + 1,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    // ─────────────── ROLLING UPDATE STATES ─────────────────────

    #[handler(
        state = CreatingNewInstanceTemplate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_instance_template(
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
                state: TriggeringRollingUpdate,
                suggested_delay: None,
            });
        }

        self.boot_check_iterations = 0;

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let group_ids: Vec<String> = self.mig_states.keys().cloned().collect();

        for group_id in &group_ids {
            if let Some(group) = config
                .capacity_groups
                .iter()
                .find(|g| &g.group_id == group_id)
            {
                if let Some(old_template) = self.instance_templates.get(group_id) {
                    self.old_instance_templates_to_delete
                        .insert(group_id.clone(), old_template.template_name.clone());
                }
                self.instance_templates.remove(group_id);

                let versioned_name = format!(
                    "{}-{}-{}-template-v{}",
                    ctx.resource_prefix, config.id, group_id, ts
                );

                self.create_instance_template_for_group(ctx, group, &config, Some(&versioned_name))
                    .await?;
            }
        }

        self.rolling_update_triggered = true;
        Ok(HandlerAction::Continue {
            state: TriggeringRollingUpdate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = TriggeringRollingUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn triggering_rolling_update(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if !self.rolling_update_triggered {
            return Ok(HandlerAction::Continue {
                state: WaitingForRollingUpdateComplete,
                suggested_delay: None,
            });
        }

        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for (group_id, mig_state) in &self.mig_states {
            if let (Some(mig_name), Some(zone), Some(template_state)) = (
                &mig_state.mig_name,
                &mig_state.zone,
                self.instance_templates.get(group_id),
            ) {
                info!(mig_name = %mig_name, new_template = %template_state.template_name, "Triggering PROACTIVE rolling update");

                let patch = InstanceGroupManager::builder()
                    .instance_template(template_state.self_link.clone())
                    .update_policy(InstanceGroupManagerUpdatePolicy {
                        r#type: Some(UpdatePolicyType::Proactive),
                        minimal_action: Some(MinimalAction::Replace),
                        most_disruptive_allowed_action: None,
                        max_surge: Some(FixedOrPercent {
                            fixed: Some(1),
                            percent: None,
                            calculated: None,
                        }),
                        max_unavailable: Some(FixedOrPercent {
                            fixed: Some(0),
                            percent: None,
                            calculated: None,
                        }),
                        replacement_method: None,
                    })
                    .build();

                compute_client
                    .patch_instance_group_manager(zone.clone(), mig_name.clone(), patch)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to trigger rolling update on MIG {}", mig_name),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
        }

        self.rolling_update_poll_iterations = 0;
        Ok(HandlerAction::Continue {
            state: WaitingForRollingUpdateComplete,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = WaitingForRollingUpdateComplete,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_rolling_update_complete(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if !self.rolling_update_triggered {
            return Ok(HandlerAction::Continue {
                state: CleanupOldInstanceTemplate,
                suggested_delay: None,
            });
        }

        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        const ROLLING_UPDATE_TIMEOUT: u32 = 60; // ~30 min at 30s intervals

        let mut all_updated = true;

        for (group_id, mig_state) in &self.mig_states {
            if let (Some(mig_name), Some(zone), Some(template_state)) = (
                &mig_state.mig_name,
                &mig_state.zone,
                self.instance_templates.get(group_id),
            ) {
                let instances = compute_client
                    .list_managed_instances(zone.clone(), mig_name.clone())
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to list instances for MIG {}", mig_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                let template_suffix = format!("/{}", template_state.template_name);
                let all_on_new = instances.managed_instances.iter().all(|inst| {
                    let no_action = matches!(
                        inst.current_action,
                        None | Some(ManagedInstanceCurrentAction::None)
                    );
                    let on_new = inst
                        .version
                        .as_ref()
                        .and_then(|v| v.instance_template.as_deref())
                        .map(|t| t.ends_with(&template_suffix))
                        .unwrap_or(false);
                    no_action && on_new
                });

                if !all_on_new {
                    all_updated = false;
                    debug!(mig_name = %mig_name, "Rolling update still in progress");
                }
            }
        }

        if all_updated {
            info!("Rolling update complete — all instances on new template");
            self.rolling_update_triggered = false;
            return Ok(HandlerAction::Continue {
                state: CleanupOldInstanceTemplate,
                suggested_delay: None,
            });
        }

        self.rolling_update_poll_iterations += 1;
        if self.rolling_update_poll_iterations >= ROLLING_UPDATE_TIMEOUT {
            let boot_log =
                Self::collect_serial_port_output(&*compute_client, &self.mig_states).await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "Rolling update did not complete after {} iterations (~30 min).\n{}",
                    ROLLING_UPDATE_TIMEOUT,
                    super::summarize_boot_log(&boot_log.unwrap_or_default()),
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        Ok(HandlerAction::Continue {
            state: WaitingForRollingUpdateComplete,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = CleanupOldInstanceTemplate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn cleanup_old_instance_template(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.old_instance_templates_to_delete.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        let old_templates: Vec<(String, String)> =
            self.old_instance_templates_to_delete.drain().collect();
        for (group_id, old_name) in old_templates {
            info!(template_name = %old_name, group_id = %group_id, "Deleting old instance template");
            match compute_client
                .delete_instance_template(old_name.clone())
                .await
            {
                Ok(_) => info!(template_name = %old_name, "Old template deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(template_name = %old_name, "Old template already deleted");
                }
                Err(e) => {
                    warn!(template_name = %old_name, error = ?e, "Failed to delete old template, continuing");
                }
            }
        }

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
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(cluster_id = %config.id, "Starting GCP ContainerCluster deletion");

        Ok(HandlerAction::Continue {
            state: DeletingManagedInstanceGroups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingManagedInstanceGroups,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_managed_instance_groups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for state in self.mig_states.values() {
            if let (Some(mig_name), Some(zone)) = (&state.mig_name, &state.zone) {
                info!(mig_name = %mig_name, "Deleting Managed Instance Group");

                match compute_client
                    .delete_instance_group_manager(zone.clone(), mig_name.clone())
                    .await
                {
                    Ok(_) => info!(mig_name = %mig_name, "MIG deleted"),
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        info!(mig_name = %mig_name, "MIG already deleted")
                    }
                    Err(e) => {
                        return Err(e).context(ErrorData::CloudPlatformError {
                            message: format!("Failed to delete MIG {}", mig_name),
                            resource_id: None,
                        })
                    }
                }
            }
        }

        self.mig_states.clear();

        Ok(HandlerAction::Continue {
            state: DeletingFirewallRules,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = DeletingFirewallRules,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_firewall_rules(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(firewall_name) = &self.firewall_rule_name {
            info!(firewall_name = %firewall_name, "Deleting firewall rule");

            match compute_client.delete_firewall(firewall_name.clone()).await {
                Ok(_) => info!(firewall_name = %firewall_name, "Firewall rule deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(firewall_name = %firewall_name, "Firewall rule already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete firewall rule {}", firewall_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.firewall_rule_name = None;

        if let Some(hc_firewall_name) = &self.health_check_firewall_rule_name {
            info!(firewall_name = %hc_firewall_name, "Deleting health check firewall rule");

            match compute_client
                .delete_firewall(hc_firewall_name.clone())
                .await
            {
                Ok(_) => {
                    info!(firewall_name = %hc_firewall_name, "Health check firewall rule deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(firewall_name = %hc_firewall_name, "Health check firewall rule already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete health check firewall rule {}",
                            hc_firewall_name
                        ),
                        resource_id: None,
                    })
                }
            }
        }

        self.health_check_firewall_rule_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingInstanceTemplate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingInstanceTemplate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_instance_template(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        for (group_id, template_state) in &self.instance_templates {
            info!(
                template_name = %template_state.template_name,
                group_id = %group_id,
                "Deleting instance template"
            );

            match compute_client
                .delete_instance_template(template_state.template_name.clone())
                .await
            {
                Ok(_) => {
                    info!(template_name = %template_state.template_name, "Instance template deleted")
                }
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(template_name = %template_state.template_name, "Instance template already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete instance template '{}' for group '{}'",
                            template_state.template_name, group_id
                        ),
                        resource_id: None,
                    })
                }
            }
        }

        self.instance_templates.clear();

        Ok(HandlerAction::Continue {
            state: DeletingHealthCheck,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingHealthCheck,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_health_check(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;

        if let Some(hc_name) = &self.health_check_name {
            info!(health_check_name = %hc_name, "Deleting health check");

            match compute_client.delete_health_check(hc_name.clone()).await {
                Ok(_) => info!(health_check_name = %hc_name, "Health check deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(health_check_name = %hc_name, "Health check already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete health check {}", hc_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.health_check_name = None;
        self.health_check_self_link = None;

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
        let gcp_cfg = ctx.get_gcp_config()?;
        let secret_manager_client = ctx
            .service_provider
            .get_gcp_secret_manager_client(gcp_cfg)?;

        if let Some(secret_name) = &self.machine_token_secret_name {
            info!(secret_name = %secret_name, "Deleting machine token secret");

            match secret_manager_client
                .delete_secret(secret_name.clone())
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

        self.machine_token_secret_name = None;

        // Also delete the OTLP auth header secrets if they were created
        for otlp_secret_name in [
            &self.otlp_auth_secret_name,
            &self.otlp_metrics_auth_secret_name,
        ]
        .into_iter()
        .flatten()
        {
            info!(secret_name = %otlp_secret_name, "Deleting OTLP secret");
            match secret_manager_client
                .delete_secret(otlp_secret_name.clone())
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

        self.otlp_auth_secret_name = None;
        self.otlp_metrics_auth_secret_name = None;

        Ok(HandlerAction::Continue {
            state: DeletingServiceAccount,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingServiceAccount,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_service_account(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let gcp_cfg = ctx.get_gcp_config()?;
        let iam_client = ctx.service_provider.get_gcp_iam_client(gcp_cfg)?;

        if let Some(sa_name) = &self.service_account_name {
            info!(service_account = %sa_name, "Deleting service account");

            match iam_client.delete_service_account(sa_name.clone()).await {
                Ok(_) => info!(service_account = %sa_name, "Service account deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(service_account = %sa_name, "Service account already deleted")
                }
                Err(e) => {
                    return Err(e).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete service account {}", sa_name),
                        resource_id: None,
                    })
                }
            }
        }

        self.service_account_name = None;
        self.service_account_email = None;

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
            .mig_states
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

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        None
    }
}

impl GcpContainerClusterController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(cluster_id: &str, capacity_groups: Vec<(&str, u32)>) -> Self {
        let mut mig_states = HashMap::new();
        for (group_id, size) in capacity_groups {
            mig_states.insert(
                group_id.to_string(),
                MigState {
                    group_id: group_id.to_string(),
                    mig_name: Some(format!("test-mig-{}", group_id)),
                    zone: Some("us-central1-a".to_string()),
                    current_size: size,
                    desired_size: size,
                    instance_type: Some("e2-medium".to_string()),
                },
            );
        }

        let mut instance_templates = HashMap::new();
        instance_templates.insert(
            "general".to_string(),
            InstanceTemplateState {
                group_id: "general".to_string(),
                template_name: "test-template-general".to_string(),
                self_link: "https://compute.googleapis.com/compute/v1/projects/test/global/instanceTemplates/test-template-general".to_string(),
            },
        );

        Self {
            state: GcpContainerClusterState::Ready,
            service_account_email: Some("test-sa@test-project.iam.gserviceaccount.com".to_string()),
            service_account_name: Some("test-sa".to_string()),
            machine_token_secret_name: Some("test-secret".to_string()),
            otlp_auth_secret_name: None,
            otlp_metrics_auth_secret_name: None,
            instance_templates,
            firewall_rule_name: Some("test-firewall".to_string()),
            health_check_firewall_rule_name: Some("test-hc-firewall".to_string()),
            health_check_name: Some("test-hc".to_string()),
            health_check_self_link: Some("https://compute.googleapis.com/compute/v1/projects/test/global/healthChecks/test-hc".to_string()),
            mig_states,
            horizon_cluster_id: Some(cluster_id.to_string()),
            horizon_ready: true,
            horizon_api_url: Some("https://horizon.example.com".to_string()),
            boot_check_iterations: 0,
            rolling_update_poll_iterations: 0,
            new_groups_pending_ready: vec![],
            old_instance_templates_to_delete: HashMap::new(),
            rolling_update_triggered: false,
            _internal_stay_count: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::controller_test::SingleControllerExecutor;
    use crate::core::MockPlatformServiceProvider;
    use alien_client_core::ErrorData as CloudClientErrorData;
    use alien_core::NetworkSettings;
    use alien_core::{
        CapacityGroup, ComputeBackend, ContainerCluster, EnvironmentVariable,
        EnvironmentVariableType, EnvironmentVariablesSnapshot, HorizonClusterConfig, HorizonConfig,
        Network, ResourceStatus,
    };
    use alien_gcp_clients::gcp::compute::MockComputeApi;
    use alien_gcp_clients::gcp::compute::{
        InstanceGroupManagersListManagedInstancesResponse, ManagedInstance,
        ManagedInstanceCurrentAction, ManagedInstanceStatus, ManagedInstanceVersion, Operation,
    };
    use alien_gcp_clients::gcp::iam::{IamPolicy, MockIamApi};
    use alien_gcp_clients::gcp::resource_manager::MockResourceManagerApi;
    use alien_gcp_clients::gcp::secret_manager::{MockSecretManagerApi, SecretVersion};
    use httpmock::prelude::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn setup_mock_provider(
        compute: Arc<MockComputeApi>,
        iam: Arc<MockIamApi>,
        resource_manager: Arc<MockResourceManagerApi>,
        secret_manager: Arc<MockSecretManagerApi>,
    ) -> Arc<MockPlatformServiceProvider> {
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_compute_client()
            .returning(move |_| Ok(compute.clone()));
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        provider
            .expect_get_gcp_resource_manager_client()
            .returning(move |_| Ok(resource_manager.clone()));
        provider
            .expect_get_gcp_secret_manager_client()
            .returning(move |_| Ok(secret_manager.clone()));
        Arc::new(provider)
    }

    fn mock_gcp_clients_for_create_delete() -> (
        Arc<MockComputeApi>,
        Arc<MockIamApi>,
        Arc<MockResourceManagerApi>,
        Arc<MockSecretManagerApi>,
    ) {
        let mut compute = MockComputeApi::new();
        let mut iam = MockIamApi::new();
        let mut resource_manager = MockResourceManagerApi::new();
        let mut secret_manager = MockSecretManagerApi::new();

        iam.expect_get_service_account().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "ServiceAccount".to_string(),
                    resource_name: "missing".to_string(),
                },
            ))
        });
        iam.expect_create_service_account().returning(|_, _| {
            Ok(ServiceAccount {
                email: Some("test-sa@test-project.iam.gserviceaccount.com".to_string()),
                name: Some("projects/test/serviceAccounts/test-sa".to_string()),
                ..Default::default()
            })
        });
        iam.expect_delete_service_account().returning(|_| Ok(()));

        resource_manager
            .expect_get_project_iam_policy()
            .returning(|_, _| {
                Ok(IamPolicy {
                    version: Some(1),
                    kind: None,
                    resource_id: None,
                    bindings: vec![],
                    etag: None,
                })
            });
        resource_manager
            .expect_set_project_iam_policy()
            .returning(|_, _, _| {
                Ok(IamPolicy {
                    version: Some(1),
                    kind: None,
                    resource_id: None,
                    bindings: vec![],
                    etag: None,
                })
            });

        secret_manager.expect_get_secret().returning(|_| {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Secret".to_string(),
                    resource_name: "missing".to_string(),
                },
            ))
        });
        secret_manager.expect_create_secret().returning(|_, _| {
            Ok(Secret {
                name: Some("projects/test/secrets/test-secret".to_string()),
                ..Default::default()
            })
        });
        secret_manager
            .expect_add_secret_version()
            .returning(|_, _| Ok(SecretVersion::default()));
        secret_manager
            .expect_get_secret_iam_policy()
            .returning(|_| {
                Ok(IamPolicy {
                    version: Some(1),
                    kind: None,
                    resource_id: None,
                    bindings: vec![],
                    etag: None,
                })
            });
        secret_manager
            .expect_set_secret_iam_policy()
            .returning(|_, _| {
                Ok(IamPolicy {
                    version: Some(1),
                    kind: None,
                    resource_id: None,
                    bindings: vec![],
                    etag: None,
                })
            });
        secret_manager.expect_delete_secret().returning(|_| Ok(()));

        compute
            .expect_insert_instance_template()
            .returning(|_| Ok(Operation::default()));
        compute
            .expect_insert_firewall()
            .returning(|_| Ok(Operation::default()));
        compute
            .expect_insert_health_check()
            .returning(|_| Ok(Operation::default()));
        compute
            .expect_insert_instance_group_manager()
            .returning(|_, _| Ok(Operation::default()));
        compute.expect_list_managed_instances().returning(|_, _| {
            Ok(InstanceGroupManagersListManagedInstancesResponse {
                managed_instances: vec![ManagedInstance {
                    instance_status: Some(ManagedInstanceStatus::Running),
                    ..Default::default()
                }],
                next_page_token: None,
            })
        });
        compute
            .expect_resize_instance_group_manager()
            .returning(|_, _, _| Ok(Operation::default()));
        compute
            .expect_delete_instance_group_manager()
            .returning(|_, _| Ok(Operation::default()));
        compute
            .expect_delete_firewall()
            .returning(|_| Ok(Operation::default()));
        compute
            .expect_delete_instance_template()
            .returning(|_| Ok(Operation::default()));
        compute
            .expect_delete_health_check()
            .returning(|_| Ok(Operation::default()));

        (
            Arc::new(compute),
            Arc::new(iam),
            Arc::new(resource_manager),
            Arc::new(secret_manager),
        )
    }

    fn mock_gcp_clients_for_best_effort_delete() -> (
        Arc<MockComputeApi>,
        Arc<MockIamApi>,
        Arc<MockResourceManagerApi>,
        Arc<MockSecretManagerApi>,
    ) {
        let mut compute = MockComputeApi::new();
        let mut iam = MockIamApi::new();
        let resource_manager = MockResourceManagerApi::new();
        let mut secret_manager = MockSecretManagerApi::new();

        iam.expect_delete_service_account().returning(|_| Ok(()));

        secret_manager.expect_delete_secret().returning(|_| Ok(()));

        let not_found = || {
            Err(AlienError::new(
                CloudClientErrorData::RemoteResourceNotFound {
                    resource_type: "Compute".to_string(),
                    resource_name: "missing".to_string(),
                },
            ))
        };

        compute
            .expect_delete_instance_group_manager()
            .returning(move |_, _| not_found());
        compute
            .expect_delete_firewall()
            .returning(move |_| not_found());
        compute
            .expect_delete_instance_template()
            .returning(move |_| not_found());
        compute
            .expect_delete_health_check()
            .returning(move |_| not_found());

        (
            Arc::new(compute),
            Arc::new(iam),
            Arc::new(resource_manager),
            Arc::new(secret_manager),
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
                instance_type: Some("e2-medium".to_string()),
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

        let (compute, iam, resource_manager, secret_manager) = mock_gcp_clients_for_create_delete();
        let mock_provider = setup_mock_provider(compute, iam, resource_manager, secret_manager);

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

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_cluster())
            .controller(GcpContainerClusterController::default())
            .platform(alien_core::Platform::Gcp)
            .compute_backend(compute_backend)
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
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

        // Mock the Horizon API for SyncingHorizonCapacityGroups
        let horizon_server = MockServer::start_async().await;
        horizon_server.mock(|when, then| {
            when.method(httpmock::Method::PATCH)
                .path(format!("/clusters/{}", cluster_id));
            then.status(200)
                .json_body(serde_json::json!({"success": true}));
        });

        let (compute, iam, resource_manager, secret_manager) = mock_gcp_clients_for_create_delete();
        let mock_provider = setup_mock_provider(compute, iam, resource_manager, secret_manager);

        let mut ready_controller =
            GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);
        ready_controller.horizon_api_url = Some(horizon_server.base_url());

        // Use matching template_inputs so rolling update is not triggered
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
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
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
        let (compute, iam, resource_manager, secret_manager) =
            mock_gcp_clients_for_best_effort_delete();
        let mock_provider = setup_mock_provider(compute, iam, resource_manager, secret_manager);

        let ready_controller =
            GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);

        let mut executor = SingleControllerExecutor::builder()
            .resource(test_cluster())
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
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

        // Build mocks that support rolling update operations
        let mut compute = MockComputeApi::new();
        let (_, iam, resource_manager, secret_manager) = mock_gcp_clients_for_create_delete();

        // Structural update: resize existing MIG
        compute
            .expect_resize_instance_group_manager()
            .returning(|_, _, _| Ok(Operation::default()));

        // Capture the template name from insert to use in list_managed_instances
        let captured_template = Arc::new(std::sync::Mutex::new(String::new()));
        let ct_write = captured_template.clone();
        compute
            .expect_insert_instance_template()
            .returning(move |t| {
                if let Some(name) = &t.name {
                    *ct_write.lock().unwrap() = name.clone();
                }
                Ok(Operation::default())
            });

        // Rolling update: patch MIG with new template + PROACTIVE policy
        compute
            .expect_patch_instance_group_manager()
            .returning(|_, _, _| Ok(Operation::default()));

        // Rolling update: poll instances — return all on new template with no pending action
        let ct_read = captured_template.clone();
        compute.expect_list_managed_instances()
            .returning(move |_, _| {
                let tpl_name = ct_read.lock().unwrap().clone();
                let url = format!("https://compute.googleapis.com/compute/v1/projects/test/global/instanceTemplates/{}", tpl_name);
                Ok(InstanceGroupManagersListManagedInstancesResponse {
                    managed_instances: vec![ManagedInstance {
                        instance_status: Some(ManagedInstanceStatus::Running),
                        current_action: Some(ManagedInstanceCurrentAction::None),
                        version: Some(ManagedInstanceVersion {
                            instance_template: Some(url),
                            name: None,
                        }),
                        ..Default::default()
                    }],
                    next_page_token: None,
                })
            });
        // Cleanup: delete old instance template
        compute
            .expect_delete_instance_template()
            .returning(|_| Ok(Operation::default()));

        let mock_provider =
            setup_mock_provider(Arc::new(compute), iam, resource_manager, secret_manager);

        let mut ready_controller =
            GcpContainerClusterController::mock_ready(cluster_id, vec![("general", 1)]);
        ready_controller.horizon_api_url = Some(horizon_server.base_url());

        // Original config with one template_inputs URL
        let mut original_cluster = test_cluster();
        original_cluster
            .template_inputs
            .as_mut()
            .unwrap()
            .horizon_api_url = horizon_server.base_url();

        // Updated config: same structure but DIFFERENT template_inputs to trigger rolling update
        let mut updated_cluster = original_cluster.clone();
        updated_cluster
            .template_inputs
            .as_mut()
            .unwrap()
            .horizond_download_base_url = "http://new-releases.test".to_string();

        let mut executor = SingleControllerExecutor::builder()
            .resource(original_cluster)
            .controller(ready_controller)
            .platform(alien_core::Platform::Gcp)
            .compute_backend(test_horizon_config(cluster_id))
            .environment_variables(test_env_vars())
            .service_provider(mock_provider)
            .with_dependency(
                test_network(),
                GcpNetworkController::mock_ready("default-network"),
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
