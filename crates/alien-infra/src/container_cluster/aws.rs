//! AWS ContainerCluster Controller
//!
//! This module implements the AWS-specific controller for managing ContainerCluster resources.
//! A ContainerCluster provisions the compute infrastructure for running containers via Horizon:
//!
//! - IAM role for EC2 instances (to access Parameter Store, register with ALB, etc.)
//! - Launch Template with horizond agent configuration
//! - Auto Scaling Groups (one per capacity group)
//! - Security groups for instance communication
//!
//! The platform creates the Horizon cluster via the Horizon API before deployment.
//! This controller provisions the cloud infrastructure that machines use to join the cluster.

use alien_aws_clients::autoscaling::{
    AsgTag, CreateAutoScalingGroupRequest, DeleteAutoScalingGroupRequest,
    DescribeAutoScalingGroupsRequest, DescribeInstanceRefreshesRequest,
    LaunchTemplateSpecification, RefreshPreferences, StartInstanceRefreshRequest,
    UpdateAutoScalingGroupRequest,
};
use alien_aws_clients::ec2::{
    AuthorizeSecurityGroupIngressRequest, CreateLaunchTemplateRequest,
    CreateLaunchTemplateVersionRequest, CreateSecurityGroupRequest, DeleteLaunchTemplateRequest,
    DescribeInstancesRequest, Ec2Api, Filter, IpPermission, IpRange,
    LaunchTemplateIamInstanceProfileSpecification, RequestLaunchTemplateData, Tag,
    TagSpecification,
};
use alien_aws_clients::elbv2::CreateTargetGroupRequest;
use alien_aws_clients::iam::{CreateInstanceProfileRequest, CreateRoleRequest};
use alien_aws_clients::secrets_manager::{
    CreateSecretRequest, DeleteSecretRequest, DescribeSecretRequest, SecretsManagerApi,
    UpdateSecretRequest,
};
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

use crate::core::ResourceControllerContext;
use crate::error::{ErrorData, Result};
use crate::horizon::{create_horizon_client, to_horizon_capacity_groups};
use crate::network::AwsNetworkController;
use alien_permissions::{
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    get_permission_set, BindingTarget, PermissionContext,
};

/// Tracks the state of a single Auto Scaling Group (one per capacity group).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AsgState {
    /// Capacity group ID this ASG is for
    pub group_id: String,
    /// ASG name
    pub asg_name: Option<String>,
    /// Current number of instances
    pub current_size: u32,
    /// Desired number of instances (from capacity plan)
    pub desired_size: u32,
    /// Instance type used
    pub instance_type: Option<String>,
}

/// Tracks a launch template created for a capacity group.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTemplateState {
    pub group_id: String,
    pub template_id: String,
    pub template_name: String,
}

/// AWS ContainerCluster Controller state machine.
///
/// This controller manages the lifecycle of AWS infrastructure for container workloads:
/// - IAM role and instance profile for EC2 instances
/// - Launch Template with horizond configuration
/// - Auto Scaling Groups (one per capacity group)
/// - Security groups for cluster networking
#[controller]
pub struct AwsContainerClusterController {
    // IAM resources
    pub(crate) role_name: Option<String>,
    pub(crate) role_arn: Option<String>,
    pub(crate) instance_profile_name: Option<String>,
    pub(crate) instance_profile_arn: Option<String>,

    // Launch Templates (one per capacity group)
    pub(crate) launch_templates: HashMap<String, LaunchTemplateState>,

    // Security Group
    pub(crate) security_group_id: Option<String>,

    // Health check target group (for ASG ELB health checks)
    #[serde(default)]
    pub(crate) target_group_arn: Option<String>,

    // Auto Scaling Groups (one per capacity group)
    pub(crate) asg_states: HashMap<String, AsgState>,

    // Horizon cluster info (from HorizonConfig)
    pub(crate) horizon_cluster_id: Option<String>,
    pub(crate) horizon_api_url: Option<String>,

    // Boot diagnostics: counts iterations spent waiting for instances to become healthy.
    #[serde(default)]
    pub(crate) boot_check_iterations: u32,

    /// AWS Secrets Manager secret name for the OTLP logs auth header (optional).
    #[serde(default)]
    pub(crate) otlp_auth_secret_name: Option<String>,
    /// AWS Secrets Manager secret name for the OTLP metrics auth header (optional).
    /// Only set when metrics uses a separate auth header from logs (e.g. different Axiom dataset).
    #[serde(default)]
    pub(crate) otlp_metrics_auth_secret_name: Option<String>,

    /// Groups newly created during an update flow, waiting for their instances to become healthy.
    #[serde(default)]
    pub(crate) new_groups_pending_ready: Vec<String>,

    /// Whether a rolling update was triggered in the current update cycle.
    #[serde(default)]
    pub(crate) rolling_update_triggered: bool,

    /// Counts iterations spent waiting for rolling updates to complete.
    #[serde(default)]
    pub(crate) rolling_update_poll_iterations: u32,

    /// Instance refresh IDs currently in progress, keyed by ASG name.
    #[serde(default)]
    pub(crate) instance_refresh_ids: HashMap<String, String>,
}

// BOOT_DIAG_TIMEOUT_ITERATIONS: number of 30-second polling iterations (~15 minutes).
const BOOT_DIAG_TIMEOUT_ITERATIONS: u32 = 30;

impl AwsContainerClusterController {
    async fn get_ubuntu_ami(&self, ec2_client: &dyn Ec2Api) -> Result<String> {
        let response = ec2_client
            .describe_images(
                alien_aws_clients::ec2::DescribeImagesRequest::builder()
                    .owners(vec!["099720109477".to_string()])
                    .filters(vec![
                        alien_aws_clients::ec2::Filter::builder()
                            .name("name".to_string())
                            .values(vec![
                                "ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-arm64-server-*"
                                    .to_string(),
                            ])
                            .build(),
                        alien_aws_clients::ec2::Filter::builder()
                            .name("state".to_string())
                            .values(vec!["available".to_string()])
                            .build(),
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe Ubuntu AMIs".to_string(),
                resource_id: None,
            })?;

        let images = response.images_set.map(|set| set.items).unwrap_or_default();

        let ami = images
            .into_iter()
            .max_by_key(|img| img.creation_date.clone().unwrap_or_default())
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "No Ubuntu AMI found".to_string(),
                    resource_id: None,
                })
            })?;

        ami.image_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Ubuntu AMI missing image ID".to_string(),
                resource_id: None,
            })
        })
    }

    fn machine_token_key(resource_id: &str) -> String {
        format!(
            "ALIEN_HORIZON_MACHINE_TOKEN_{}",
            resource_id.to_uppercase().replace('-', "_")
        )
    }

    fn machine_token_secret_name(resource_prefix: &str, resource_id: &str) -> String {
        let vault_prefix = format!("{}-secrets", resource_prefix);
        format!("{}-{}", vault_prefix, Self::machine_token_key(resource_id))
    }

    fn otlp_auth_secret_name_for(resource_prefix: &str) -> String {
        format!("{}-secrets-ALIEN_OTLP_AUTH_HEADER", resource_prefix)
    }

    fn otlp_metrics_auth_secret_name_for(resource_prefix: &str) -> String {
        format!("{}-secrets-ALIEN_OTLP_METRICS_AUTH_HEADER", resource_prefix)
    }

    /// Create tags for AWS resources.
    fn create_tags(&self, resource_prefix: &str, resource_type: &str) -> Vec<TagSpecification> {
        vec![TagSpecification {
            resource_type: resource_type.to_string(),
            tags: vec![
                Tag {
                    key: "Name".to_string(),
                    value: format!("{}-{}", resource_prefix, resource_type.to_lowercase()),
                },
                Tag {
                    key: "ManagedBy".to_string(),
                    value: "Alien".to_string(),
                },
            ],
        }]
    }

    /// Create or update an AWS Secrets Manager secret with the given value.
    async fn aws_upsert_secret(
        client: &dyn SecretsManagerApi,
        secret_id: &str,
        value: &str,
        description: &str,
        config: &ContainerCluster,
    ) -> Result<()> {
        match client
            .describe_secret(DescribeSecretRequest {
                secret_id: secret_id.to_string(),
            })
            .await
        {
            Ok(_) => {
                client
                    .update_secret(
                        UpdateSecretRequest::builder()
                            .secret_id(secret_id.to_string())
                            .secret_string(value.to_string())
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to update secret '{}'", secret_id),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
            Err(e)
                if matches!(
                    e.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                client
                    .create_secret(CreateSecretRequest {
                        name: secret_id.to_string(),
                        description: Some(description.to_string()),
                        secret_string: Some(value.to_string()),
                        secret_binary: None,
                        kms_key_id: None,
                        tags: None,
                        force_overwrite_replica_secret: None,
                        replica_regions: None,
                        client_request_token: None,
                    })
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create secret '{}'", secret_id),
                        resource_id: Some(config.id.clone()),
                    })?;
            }
            Err(e) => {
                return Err(e).context(ErrorData::CloudPlatformError {
                    message: format!("Failed to check secret '{}' existence", secret_id),
                    resource_id: Some(config.id.clone()),
                });
            }
        }
        Ok(())
    }

    /// Generate user data script for horizond.
    fn generate_user_data(
        &self,
        cluster_id: &str,
        api_url: &str,
        horizond_download_base_url: &str,
        capacity_group: &str,
        machine_token_secret: &str,
        otlp_logs_endpoint: Option<&str>,
        otlp_metrics_endpoint: Option<&str>,
        otlp_auth_secret_name: Option<&str>,
        otlp_metrics_auth_secret_name: Option<&str>,
    ) -> String {
        // Base64-encoded cloud-init script that:
        // 1. Fetches machine token (and optionally OTLP auth headers) from Secrets Manager
        // 2. Installs horizond dependencies
        // 3. Starts horizond with cluster configuration
        let horizond_url =
            super::join_url_path(horizond_download_base_url, "linux-aarch64/horizond");

        // Build the optional block that fetches the OTLP logs auth header from Secrets Manager.
        let otlp_secret_fetch = match otlp_auth_secret_name {
            Some(sn) => format!(
                r#"# Fetch OTLP logs auth header from Secrets Manager
OTLP_AUTH_HEADER=$(aws secretsmanager get-secret-value \
  --secret-id "{}" \
  --region "$REGION" \
  --query SecretString \
  --output text)"#,
                sn
            ),
            None => String::new(),
        };

        // Fetch the OTLP metrics auth header from a separate secret if configured.
        let otlp_metrics_secret_fetch = match otlp_metrics_auth_secret_name {
            Some(sn) => format!(
                r#"
# Fetch OTLP metrics auth header from Secrets Manager
OTLP_METRICS_AUTH_HEADER=$(aws secretsmanager get-secret-value \
  --secret-id "{}" \
  --region "$REGION" \
  --query SecretString \
  --output text)"#,
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

        let script = format!(
            r#"#!/bin/bash
set -euo pipefail

log() {{ echo "[HORIZON-BOOT] $1"; }}
trap 'log "error: script failed at line $LINENO (exit $?)"' ERR

log "packages_installing"
if command -v apt-get >/dev/null 2>&1; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq containerd wireguard-tools curl awscli jq tar
elif command -v dnf >/dev/null 2>&1; then
  dnf install -y -q containerd wireguard-tools curl awscli jq tar
elif command -v yum >/dev/null 2>&1; then
  yum install -y -q containerd wireguard-tools curl awscli jq tar
fi

systemctl enable --now containerd || true
log "packages_installed"

log "dependencies_installing"
mkdir -p /opt/cni/bin /etc/cni/net.d /etc/wireguard /etc/horizond /var/lib/horizond
UARCH=$(uname -m)
if [ "$UARCH" = "x86_64" ]; then ARCH="amd64"; else ARCH="arm64"; fi
curl -fsSL "https://github.com/containernetworking/plugins/releases/download/v1.4.0/cni-plugins-linux-${{ARCH}}-v1.4.0.tgz" | tar -C /opt/cni/bin -xz
curl -fsSL "https://github.com/containerd/nerdctl/releases/download/v1.7.2/nerdctl-1.7.2-linux-${{ARCH}}.tar.gz" | tar -C /usr/local/bin -xz
log "dependencies_installed"

TOKEN=$(curl -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600" -s)
INSTANCE_ID=$(curl -H "X-aws-ec2-metadata-token: $TOKEN" -s http://169.254.169.254/latest/meta-data/instance-id)
ZONE=$(curl -H "X-aws-ec2-metadata-token: $TOKEN" -s http://169.254.169.254/latest/meta-data/placement/availability-zone)
REGION=$(echo "$ZONE" | sed 's/[a-z]$//')

# Fetch machine token from Secrets Manager
MACHINE_TOKEN=$(aws secretsmanager get-secret-value \
  --secret-id "{}" \
  --region "$REGION" \
  --query SecretString \
  --output text)
{}

CAPACITY_GROUP=$(aws ec2 describe-tags \
  --region "$REGION" \
  --filters "Name=resource-id,Values=$INSTANCE_ID" "Name=key,Values=CapacityGroup" \
  --query 'Tags[0].Value' \
  --output text)
if [ -z "$CAPACITY_GROUP" ] || [ "$CAPACITY_GROUP" = "None" ]; then
  CAPACITY_GROUP="{}"
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
  --machine-id $INSTANCE_ID \\
  --machine-token $MACHINE_TOKEN \\
  --api-url "{}" \\
  --zone $ZONE \\
  --network-interface ens5 \\
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
            machine_token_secret,
            format!("{}{}", otlp_secret_fetch, otlp_metrics_secret_fetch),
            capacity_group,
            horizond_url,
            cluster_id,
            api_url,
            if otlp_flags.is_empty() {
                String::new()
            } else {
                let trimmed = otlp_flags.trim_end_matches(" \\").trim_end_matches('\\');
                format!(" \\\n{}", trimmed)
            }
        );

        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            script.as_bytes(),
        )
    }

    /// Creates a launch template for a single capacity group.
    async fn create_launch_template_for_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group: &CapacityGroup,
        config: &ContainerCluster,
    ) -> Result<()> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let instance_profile_arn = self.instance_profile_arn.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Instance profile ARN not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let sg_id = self.security_group_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Security group ID not set".to_string(),
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
                message: "Missing template_inputs".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let otlp_auth = self.otlp_auth_secret_name.clone();
        let otlp_metrics_auth = self.otlp_metrics_auth_secret_name.clone();
        let instance_type = group.instance_type.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Group '{}': instance_type not set", group.group_id),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let ami_id = self.get_ubuntu_ami(ec2_client.as_ref()).await?;
        let template_name = format!(
            "{}-{}-{}-lt",
            ctx.resource_prefix, config.id, group.group_id
        );
        let machine_token_secret = Self::machine_token_secret_name(ctx.resource_prefix, &config.id);
        let user_data = self.generate_user_data(
            &cluster_id,
            &api_url,
            &template_inputs.horizond_download_base_url,
            &group.group_id,
            &machine_token_secret,
            template_inputs.monitoring_logs_endpoint.as_deref(),
            template_inputs.monitoring_metrics_endpoint.as_deref(),
            otlp_auth.as_deref(),
            otlp_metrics_auth.as_deref(),
        );

        let create_response = ec2_client
            .create_launch_template(
                CreateLaunchTemplateRequest::builder()
                    .launch_template_name(template_name.clone())
                    .launch_template_data(
                        RequestLaunchTemplateData::builder()
                            .image_id(ami_id)
                            .instance_type(instance_type.clone())
                            .iam_instance_profile(
                                LaunchTemplateIamInstanceProfileSpecification::builder()
                                    .arn(instance_profile_arn)
                                    .build(),
                            )
                            .security_group_ids(vec![sg_id])
                            .user_data(user_data)
                            .build(),
                    )
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "launch-template"))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!(
                    "Failed to create launch template for group '{}'",
                    group.group_id
                ),
                resource_id: Some(config.id.clone()),
            })?;

        let template_id = create_response
            .launch_template
            .and_then(|lt| lt.launch_template_id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Launch template created but no ID returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.launch_templates.insert(
            group.group_id.clone(),
            LaunchTemplateState {
                group_id: group.group_id.clone(),
                template_id,
                template_name,
            },
        );
        Ok(())
    }

    /// Creates an Auto Scaling Group for a single capacity group.
    async fn create_asg_for_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group: &CapacityGroup,
        config: &ContainerCluster,
    ) -> Result<()> {
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;
        let subnet_ids = if !network.private_subnet_ids.is_empty()
            && (network.is_byo_vpc || network.nat_gateway_id.is_some())
        {
            &network.private_subnet_ids
        } else {
            &network.public_subnet_ids
        };
        if subnet_ids.is_empty() {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No subnets available".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
        let vpc_zone_identifier = subnet_ids.join(",");
        let asg_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);
        let template = self.launch_templates.get(&group.group_id).ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("No launch template for group '{}'", group.group_id),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let instance_type = group.instance_type.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: format!("Group '{}': instance_type not set", group.group_id),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let asg_request = CreateAutoScalingGroupRequest {
            auto_scaling_group_name: asg_name.clone(),
            min_size: group.min_size as i32,
            max_size: group.max_size as i32,
            desired_capacity: Some(group.min_size as i32),
            launch_template: Some(
                LaunchTemplateSpecification::builder()
                    .launch_template_id(template.template_id.clone())
                    .version("$Latest".to_string())
                    .build(),
            ),
            launch_configuration_name: None,
            vpc_zone_identifier: Some(vpc_zone_identifier),
            availability_zones: None,
            default_cooldown: None,
            health_check_grace_period: None,
            health_check_type: self.target_group_arn.as_ref().map(|_| "ELB".to_string()),
            target_group_arns: self.target_group_arn.as_ref().map(|arn| vec![arn.clone()]),
            service_linked_role_arn: None,
            tags: Some(vec![
                AsgTag::builder()
                    .key("Name".to_string())
                    .value(asg_name.clone())
                    .propagate_at_launch(true)
                    .build(),
                AsgTag::builder()
                    .key("ManagedBy".to_string())
                    .value("Alien".to_string())
                    .propagate_at_launch(true)
                    .build(),
                AsgTag::builder()
                    .key("CapacityGroup".to_string())
                    .value(group.group_id.clone())
                    .propagate_at_launch(true)
                    .build(),
            ]),
            capacity_rebalance: None,
            default_instance_warmup: None,
            new_instances_protected_from_scale_in: None,
        };
        asg_client
            .create_auto_scaling_group(asg_request)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create ASG for group '{}'", group.group_id),
                resource_id: Some(config.id.clone()),
            })?;

        self.asg_states.insert(
            group.group_id.clone(),
            AsgState {
                group_id: group.group_id.clone(),
                asg_name: Some(asg_name),
                current_size: 0,
                desired_size: group.min_size,
                instance_type: Some(instance_type),
            },
        );
        Ok(())
    }

    /// Deletes the ASG and launch template for a capacity group (best-effort).
    async fn delete_capacity_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        group_id: &str,
        resource_id: &str,
    ) -> Result<()> {
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        if let Some(state) = self.asg_states.remove(group_id) {
            if let Some(asg_name) = state.asg_name {
                let _ = asg_client
                    .delete_auto_scaling_group(
                        DeleteAutoScalingGroupRequest::builder()
                            .auto_scaling_group_name(asg_name.clone())
                            .force_delete(true)
                            .build(),
                    )
                    .await;
                info!(asg_name = %asg_name, "Deleted ASG for removed capacity group");
            }
        }
        if let Some(tmpl) = self.launch_templates.remove(group_id) {
            let _ = ec2_client
                .delete_launch_template(
                    DeleteLaunchTemplateRequest::builder()
                        .launch_template_id(tmpl.template_id.clone())
                        .build(),
                )
                .await;
            info!(template_id = %tmpl.template_id, "Deleted launch template for removed capacity group");
        }
        let _ = resource_id;
        Ok(())
    }

    /// Tries to fetch console output from one ASG instance for boot diagnostics.
    async fn collect_console_output(
        ec2_client: &dyn Ec2Api,
        asg_states: &HashMap<String, AsgState>,
    ) -> Option<String> {
        let asg_names: Vec<String> = asg_states
            .values()
            .filter_map(|s| s.asg_name.clone())
            .collect();
        if asg_names.is_empty() {
            return None;
        }

        // Find a pending/running instance belonging to one of our ASGs.
        let describe_response = ec2_client
            .describe_instances(
                DescribeInstancesRequest::builder()
                    .filters(vec![
                        Filter::builder()
                            .name("instance-state-name".to_string())
                            .values(vec!["pending".to_string(), "running".to_string()])
                            .build(),
                        Filter::builder()
                            .name("tag:aws:autoscaling:groupName".to_string())
                            .values(asg_names)
                            .build(),
                    ])
                    .max_results(1)
                    .build(),
            )
            .await
            .ok()?;

        let instance_id = describe_response
            .reservation_set?
            .items
            .into_iter()
            .next()?
            .instances_set?
            .items
            .into_iter()
            .next()?
            .instance_id?;

        let console = ec2_client.get_console_output(instance_id).await.ok()?;

        console.decode_output()
    }
}

#[controller]
impl AwsContainerClusterController {
    // ─────────────── CREATE FLOW ──────────────────────────────

    #[flow_entry(Create)]
    #[handler(
        state = CreateStart,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(cluster_id = %config.id, "Starting ContainerCluster provisioning");

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

        Ok(HandlerAction::Continue {
            state: CreatingIamRole,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingIamRole,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_iam_role(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let role_name = format!("{}-{}-role", ctx.resource_prefix, config.id);

        info!(role_name = %role_name, "Creating IAM role for container instances");

        // Trust policy allowing EC2 to assume this role
        let assume_role_policy = serde_json::json!({
            "Version": "2012-10-17",
            "Statement": [{
                "Effect": "Allow",
                "Principal": {
                    "Service": "ec2.amazonaws.com"
                },
                "Action": "sts:AssumeRole"
            }]
        })
        .to_string();

        let create_response = iam_client
            .create_role(
                CreateRoleRequest::builder()
                    .role_name(role_name.clone())
                    .assume_role_policy_document(assume_role_policy)
                    .description(format!(
                        "IAM role for Alien ContainerCluster {} instances",
                        config.id
                    ))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create IAM role".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let role_arn = create_response.create_role_result.role.arn.clone();

        self.role_name = Some(role_name.clone());
        self.role_arn = Some(role_arn.clone());

        info!(role_arn = %role_arn, "IAM role created, attaching policies");

        // Attach required AWS managed policies for VM baseline capabilities.
        // ECR image pull is intentionally NOT here — it is applied below via the
        // container-cluster/execute permission set, which grants cross-account pull.
        let policies = vec![
            "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore", // For Parameter Store access
            "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy",  // For CloudWatch logs/metrics
            "arn:aws:iam::aws:policy/SecretsManagerReadWrite",      // For Secrets Manager access
        ];

        for policy_arn in policies {
            iam_client
                .attach_role_policy(&role_name, policy_arn)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to attach policy {} to role", policy_arn),
                    resource_id: Some(config.id.clone()),
                })?;
        }

        iam_client
            .put_role_policy(
                &role_name,
                "DescribeInstanceTags",
                r#"{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["ec2:DescribeTags"],
    "Resource": "*"
  }]
}"#,
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to attach DescribeTags policy".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        // Apply the container-cluster/execute permission set as an inline policy on the VM role.
        // This grants: cross-account ECR image pull (from managing account) + sts:AssumeRole
        // on per-container SA roles (for the IMDS metadata proxy to vend per-container credentials).
        // These are runtime permissions for the VM itself, not for the management plane.
        if let Some(execute_permission_set) = get_permission_set("container-cluster/execute") {
            let generator = AwsRuntimePermissionsGenerator::new();

            let mut permission_context = PermissionContext::new()
                .with_stack_prefix(ctx.resource_prefix.to_string())
                .with_aws_region(aws_cfg.region.clone())
                .with_aws_account_id(aws_cfg.account_id.clone());

            if let Some(aws_management) = ctx.get_aws_management_config()? {
                permission_context = permission_context
                    .with_managing_role_arn(aws_management.managing_role_arn.clone());
                if let Some(managing_account_id) =
                    PermissionContext::extract_account_id_from_role_arn(
                        &aws_management.managing_role_arn,
                    )
                {
                    permission_context =
                        permission_context.with_managing_account_id(managing_account_id);
                }
            }

            let policy = generator
                .generate_policy(
                    &execute_permission_set,
                    BindingTarget::Stack,
                    &permission_context,
                )
                .map_err(|e| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!(
                            "Failed to generate container-cluster/execute policy: {}",
                            e
                        ),
                        operation: Some("creating_iam_role".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

            if !policy.statement.is_empty() {
                let policy_doc = serde_json::to_string(&AwsIamPolicy {
                    version: "2012-10-17".to_string(),
                    statement: policy.statement,
                })
                .map_err(|e| {
                    AlienError::new(ErrorData::InfrastructureError {
                        message: format!("Failed to serialize execute policy: {}", e),
                        operation: Some("creating_iam_role".to_string()),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

                iam_client
                    .put_role_policy(&role_name, "ContainerClusterExecute", &policy_doc)
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: "Failed to apply container-cluster/execute policy to VM role"
                            .to_string(),
                        resource_id: Some(config.id.clone()),
                    })?;

                info!(role_name = %role_name, "Applied container-cluster/execute policy to VM role");
            }
        } else {
            warn!("container-cluster/execute permission set not found in registry, skipping");
        }

        Ok(HandlerAction::Continue {
            state: CreatingInstanceProfile,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingInstanceProfile,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_instance_profile(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let role_name = self.role_name.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Role name not set in state".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let profile_name = format!("{}-{}-profile", ctx.resource_prefix, config.id);

        info!(profile_name = %profile_name, "Creating instance profile");

        let create_response = iam_client
            .create_instance_profile(
                CreateInstanceProfileRequest::builder()
                    .instance_profile_name(profile_name.clone())
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create instance profile".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let profile_arn = create_response
            .create_instance_profile_result
            .instance_profile
            .arn
            .clone();

        self.instance_profile_name = Some(profile_name.clone());
        self.instance_profile_arn = Some(profile_arn.clone());

        // Add role to instance profile
        iam_client
            .add_role_to_instance_profile(&profile_name, role_name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add role to instance profile".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        info!(profile_arn = %profile_arn, "Instance profile created");

        // Wait for IAM propagation
        Ok(HandlerAction::Continue {
            state: CreatingSecurityGroup,
            suggested_delay: Some(Duration::from_secs(10)),
        })
    }

    #[handler(
        state = CreatingSecurityGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        // Get VPC from network dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;
        let vpc_id = network.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Network VPC ID not available".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let sg_name = format!("{}-{}-sg", ctx.resource_prefix, config.id);

        info!(sg_name = %sg_name, vpc_id = %vpc_id, "Creating security group for container instances");

        let create_response = ec2_client
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(sg_name.clone())
                    .description(format!(
                        "Security group for Alien ContainerCluster {} instances",
                        config.id
                    ))
                    .vpc_id(vpc_id.clone())
                    .tag_specifications(self.create_tags(ctx.resource_prefix, "security-group"))
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create security group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let sg_id = create_response.group_id.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "Security group created but no ID returned".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Add ingress rules for WireGuard mesh (UDP 51820) and internal communication
        let container_cidr = config.container_cidr();

        ec2_client
            .authorize_security_group_ingress(
                AuthorizeSecurityGroupIngressRequest::builder()
                    .group_id(sg_id.clone())
                    .ip_permissions(vec![
                        // WireGuard mesh traffic (UDP 51820)
                        IpPermission {
                            ip_protocol: "udp".to_string(),
                            from_port: Some(51820),
                            to_port: Some(51820),
                            ip_ranges: Some(vec![IpRange {
                                cidr_ip: "0.0.0.0/0".to_string(),
                                description: Some("WireGuard mesh".to_string()),
                            }]),
                            ipv6_ranges: None,
                            user_id_group_pairs: None,
                        },
                        // Container network traffic
                        IpPermission {
                            ip_protocol: "-1".to_string(),
                            from_port: None,
                            to_port: None,
                            ip_ranges: Some(vec![IpRange {
                                cidr_ip: container_cidr.to_string(),
                                description: Some("Container network".to_string()),
                            }]),
                            ipv6_ranges: None,
                            user_id_group_pairs: None,
                        },
                    ])
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to add ingress rules to security group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        self.security_group_id = Some(sg_id.clone());

        info!(sg_id = %sg_id, "Security group created");

        Ok(HandlerAction::Continue {
            state: CreatingLaunchTemplate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingLaunchTemplate,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_launch_template(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let instance_profile_arn = self.instance_profile_arn.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Instance profile ARN not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let sg_id = self.security_group_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Security group ID not set".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        // Lazily read Horizon config if not already set.
        // When imported from CF, CreateStart is skipped so horizon_cluster_id/api_url are None.
        // The controller reads them here from config.template_inputs and compute_backend.
        if self.horizon_cluster_id.is_none() {
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
        }

        let cluster_id = self.horizon_cluster_id.as_ref().unwrap();
        let api_url = self.horizon_api_url.as_ref().unwrap();

        // Update Horizon cluster with capacity groups (they were empty at creation time).
        // The platform creates clusters before deployment starts (to get tokens), so capacity
        // groups are only known now after preflights have resolved them.
        {
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
        }

        let template_inputs = config.template_inputs.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "ContainerCluster is missing template_inputs (stamp_template_inputs did not run)".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;
        let horizond_download_base_url = template_inputs.horizond_download_base_url.clone();

        let ami_id = self.get_ubuntu_ami(ec2_client.as_ref()).await?;

        // If OTLP monitoring is configured, store the auth headers in Secrets Manager
        // so the startup script can fetch them at boot — same pattern as the machine token.
        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let secrets_client = ctx
                .service_provider
                .get_aws_secrets_manager_client(aws_cfg).await?;

            let logs_secret_id = Self::otlp_auth_secret_name_for(ctx.resource_prefix);
            Self::aws_upsert_secret(
                secrets_client.as_ref(),
                &logs_secret_id,
                &monitoring.logs_auth_header,
                "Alien OTLP logs auth header for horizond agents",
                &config,
            )
            .await?;
            self.otlp_auth_secret_name = Some(logs_secret_id);

            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_id =
                    Self::otlp_metrics_auth_secret_name_for(ctx.resource_prefix);
                Self::aws_upsert_secret(
                    secrets_client.as_ref(),
                    &metrics_secret_id,
                    metrics_auth_header,
                    "Alien OTLP metrics auth header for horizond agents",
                    &config,
                )
                .await?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_id);
            }

            info!("OTLP auth headers stored in Secrets Manager");
        }

        for group in &config.capacity_groups {
            let template_name = format!(
                "{}-{}-{}-lt",
                ctx.resource_prefix, config.id, group.group_id
            );
            let machine_token_secret =
                Self::machine_token_secret_name(ctx.resource_prefix, &config.id);
            let user_data = self.generate_user_data(
                cluster_id,
                api_url,
                &horizond_download_base_url,
                &group.group_id,
                &machine_token_secret,
                template_inputs.monitoring_logs_endpoint.as_deref(),
                template_inputs.monitoring_metrics_endpoint.as_deref(),
                self.otlp_auth_secret_name.as_deref(),
                self.otlp_metrics_auth_secret_name.as_deref(),
            );
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
                template_name = %template_name,
                group_id = %group.group_id,
                "Creating launch template"
            );

            let create_response = ec2_client
                .create_launch_template(
                    CreateLaunchTemplateRequest::builder()
                        .launch_template_name(template_name.clone())
                        .launch_template_data(
                            RequestLaunchTemplateData::builder()
                                .image_id(ami_id.clone())
                                .instance_type(instance_type.clone())
                                .iam_instance_profile(
                                    LaunchTemplateIamInstanceProfileSpecification::builder()
                                        .arn(instance_profile_arn.clone())
                                        .build(),
                                )
                                .security_group_ids(vec![sg_id.clone()])
                                .user_data(user_data)
                                .build(),
                        )
                        .tag_specifications(
                            self.create_tags(ctx.resource_prefix, "launch-template"),
                        )
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to create launch template".to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let template_id = create_response
                .launch_template
                .and_then(|lt| lt.launch_template_id)
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudPlatformError {
                        message: "Launch template created but no ID returned".to_string(),
                        resource_id: Some(config.id.clone()),
                    })
                })?;

            self.launch_templates.insert(
                group.group_id.clone(),
                LaunchTemplateState {
                    group_id: group.group_id.clone(),
                    template_id: template_id.clone(),
                    template_name: template_name.clone(),
                },
            );

            info!(template_id = %template_id, "Launch template created");
        }

        Ok(HandlerAction::Continue {
            state: CreatingHealthCheckTargetGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingHealthCheckTargetGroup,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_health_check_target_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;
        let vpc_id = network.vpc_id.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "VPC ID not available from Network dependency".to_string(),
                resource_id: Some(config.id.clone()),
            })
        })?;

        let tg_name = format!("{}-{}-hc-tg", ctx.resource_prefix, config.id);
        // AWS target group names are max 32 chars; truncate if needed
        let tg_name = if tg_name.len() > 32 {
            tg_name[..32].to_string()
        } else {
            tg_name
        };

        info!(target_group_name = %tg_name, "Creating health check target group for ASG ELB health checks");

        let response = elbv2_client
            .create_target_group(
                CreateTargetGroupRequest::builder()
                    .name(tg_name.clone())
                    .target_type("instance".to_string())
                    .protocol("HTTP".to_string())
                    .port(8080)
                    .vpc_id(vpc_id.clone())
                    .health_check_enabled(true)
                    .health_check_protocol("HTTP".to_string())
                    .health_check_port("8080".to_string())
                    .health_check_path("/health".to_string())
                    .health_check_interval_seconds(30)
                    .health_check_timeout_seconds(10)
                    .healthy_threshold_count(2)
                    .unhealthy_threshold_count(3)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to create health check target group".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let tg_arn = response
            .create_target_group_result
            .target_groups
            .and_then(|w| w.members.into_iter().next())
            .and_then(|tg| tg.target_group_arn)
            .ok_or_else(|| {
                AlienError::new(ErrorData::CloudPlatformError {
                    message: "Target group created but no ARN returned".to_string(),
                    resource_id: Some(config.id.clone()),
                })
            })?;

        self.target_group_arn = Some(tg_arn.clone());
        info!(target_group_arn = %tg_arn, "Health check target group created");

        Ok(HandlerAction::Continue {
            state: CreatingAutoScalingGroups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingAutoScalingGroups,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn creating_auto_scaling_groups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        // Get subnets from network dependency
        let network_ref = ResourceRef::new(Network::RESOURCE_TYPE, "default-network".to_string());
        let network = ctx.require_dependency::<AwsNetworkController>(&network_ref)?;
        // Use private subnets only when egress is handled externally (NAT or customer VPC).
        // UseDefault has no private subnets, so VMs land in public subnets with auto-assigned IPs.
        // Create always provisions a NAT gateway, so nat_gateway_id will be set.
        // BYO: customer manages egress; use their private subnets.
        let subnet_ids = if !network.private_subnet_ids.is_empty()
            && (network.is_byo_vpc || network.nat_gateway_id.is_some())
        {
            &network.private_subnet_ids
        } else {
            &network.public_subnet_ids
        };

        if subnet_ids.is_empty() {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "No subnets available from Network".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }

        let vpc_zone_identifier = subnet_ids.join(",");

        info!(
            capacity_groups = config.capacity_groups.len(),
            "Creating Auto Scaling Groups for capacity groups"
        );

        for group in &config.capacity_groups {
            let asg_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, group.group_id);
            let template = self.launch_templates.get(&group.group_id).ok_or_else(|| {
                AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: format!(
                        "Launch template not set for capacity group '{}'",
                        group.group_id
                    ),
                    resource_id: Some(config.id.clone()),
                })
            })?;

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
                asg_name = %asg_name,
                group_id = %group.group_id,
                instance_type = %instance_type,
                min_size = group.min_size,
                max_size = group.max_size,
                "Creating Auto Scaling Group"
            );

            let asg_request = CreateAutoScalingGroupRequest {
                auto_scaling_group_name: asg_name.clone(),
                min_size: group.min_size as i32,
                max_size: group.max_size as i32,
                desired_capacity: Some(group.min_size as i32),
                launch_template: Some(
                    LaunchTemplateSpecification::builder()
                        .launch_template_id(template.template_id.clone())
                        .version("$Latest".to_string())
                        .build(),
                ),
                launch_configuration_name: None,
                vpc_zone_identifier: Some(vpc_zone_identifier.clone()),
                availability_zones: None,
                default_cooldown: None,
                health_check_grace_period: None,
                health_check_type: self.target_group_arn.as_ref().map(|_| "ELB".to_string()),
                target_group_arns: self.target_group_arn.as_ref().map(|arn| vec![arn.clone()]),
                service_linked_role_arn: None,
                tags: Some(vec![
                    AsgTag::builder()
                        .key("Name".to_string())
                        .value(asg_name.clone())
                        .propagate_at_launch(true)
                        .build(),
                    AsgTag::builder()
                        .key("ManagedBy".to_string())
                        .value("Alien".to_string())
                        .propagate_at_launch(true)
                        .build(),
                    AsgTag::builder()
                        .key("CapacityGroup".to_string())
                        .value(group.group_id.clone())
                        .propagate_at_launch(true)
                        .build(),
                ]),
                capacity_rebalance: None,
                default_instance_warmup: None,
                new_instances_protected_from_scale_in: None,
            };

            asg_client
                .create_auto_scaling_group(asg_request)
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create ASG for capacity group {}", group.group_id),
                    resource_id: Some(config.id.clone()),
                })?;

            self.asg_states.insert(
                group.group_id.clone(),
                AsgState {
                    group_id: group.group_id.clone(),
                    asg_name: Some(asg_name),
                    current_size: 0,
                    desired_size: group.min_size,
                    instance_type: Some(instance_type),
                },
            );
        }

        info!("All Auto Scaling Groups created");

        Ok(HandlerAction::Continue {
            state: WaitingForInstances,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    #[handler(
        state = WaitingForInstances,
        on_failure = CreateFailed,
        status = ResourceStatus::Provisioning,
    )]
    async fn waiting_for_instances(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let asg_names: Vec<String> = self
            .asg_states
            .values()
            .filter_map(|s| s.asg_name.clone())
            .collect();

        if asg_names.is_empty() {
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        let describe_response = asg_client
            .describe_auto_scaling_groups(
                DescribeAutoScalingGroupsRequest::builder()
                    .auto_scaling_group_names(asg_names)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe Auto Scaling Groups".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let mut all_healthy = true;
        let mut total_instances = 0u32;

        let groups = describe_response
            .describe_auto_scaling_groups_result
            .auto_scaling_groups
            .map(|w| w.members)
            .unwrap_or_default();

        for asg in groups {
            let asg_name = asg.auto_scaling_group_name.unwrap_or_default();
            let instances = asg.instances.map(|w| w.members).unwrap_or_default();
            let healthy_count = instances
                .iter()
                .filter(|i| i.health_status.as_deref() == Some("Healthy"))
                .count();

            let desired = asg.desired_capacity.unwrap_or(0) as usize;

            debug!(
                asg_name = %asg_name,
                healthy = healthy_count,
                desired = desired,
                "ASG instance status"
            );

            if healthy_count < desired {
                all_healthy = false;
            }

            total_instances += healthy_count as u32;

            // Update state
            if let Some(state) = self
                .asg_states
                .values_mut()
                .find(|s| s.asg_name.as_deref() == Some(&asg_name))
            {
                state.current_size = healthy_count as u32;
            }
        }

        if all_healthy && total_instances > 0 {
            info!(total_instances = total_instances, "All instances healthy");
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
                    let boot_log = Self::collect_console_output(
                        &*ctx.service_provider.get_aws_ec2_client(aws_cfg).await?,
                        &self.asg_states,
                    )
                    .await;
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "No instances appeared after {} iterations (~15 minutes).\n{}",
                            BOOT_DIAG_TIMEOUT_ITERATIONS,
                            super::summarize_boot_log(&boot_log.unwrap_or_default()),
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }

                debug!("Waiting for instances to launch");
                Ok(HandlerAction::Continue {
                    state: WaitingForInstances,
                    suggested_delay: Some(Duration::from_secs(30)),
                })
            }
        } else {
            self.boot_check_iterations += 1;

            if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
                let boot_log = Self::collect_console_output(
                    &*ctx.service_provider.get_aws_ec2_client(aws_cfg).await?,
                    &self.asg_states,
                )
                .await;
                return Err(AlienError::new(ErrorData::CloudPlatformError {
                    message: format!(
                        "Instances did not become healthy after {} iterations (~15 minutes).\n{}",
                        BOOT_DIAG_TIMEOUT_ITERATIONS,
                        super::summarize_boot_log(&boot_log.unwrap_or_default()),
                    ),
                    resource_id: Some(config.id.clone()),
                }));
            }

            debug!(
                total_instances = total_instances,
                iteration = self.boot_check_iterations,
                "Waiting for more instances to become healthy"
            );
            Ok(HandlerAction::Continue {
                state: WaitingForInstances,
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

        debug!(cluster_id = %config.id, "ContainerCluster ready, checking health");

        // Periodic health check - verify ASGs exist and update instance counts
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;

        let asg_names: Vec<String> = self
            .asg_states
            .values()
            .filter_map(|s| s.asg_name.clone())
            .collect();

        if !asg_names.is_empty() {
            let describe_response = asg_client
                .describe_auto_scaling_groups(
                    DescribeAutoScalingGroupsRequest::builder()
                        .auto_scaling_group_names(asg_names)
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to describe Auto Scaling Groups during health check"
                        .to_string(),
                    resource_id: Some(config.id.clone()),
                })?;

            let groups = describe_response
                .describe_auto_scaling_groups_result
                .auto_scaling_groups
                .map(|w| w.members)
                .unwrap_or_default();

            for asg in groups {
                let asg_name = asg.auto_scaling_group_name.unwrap_or_default();
                let instances = asg.instances.map(|w| w.members).unwrap_or_default();
                let healthy_count = instances
                    .iter()
                    .filter(|i| i.health_status.as_deref() == Some("Healthy"))
                    .count();

                if let Some(state) = self
                    .asg_states
                    .values_mut()
                    .find(|s| s.asg_name.as_deref() == Some(&asg_name))
                {
                    state.current_size = healthy_count as u32;
                }
            }
        }

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
        info!(cluster_id = %config.id, "AWS ContainerCluster update requested");
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
        let aws_cfg = ctx.get_aws_config()?;
        let secrets_client = ctx
            .service_provider
            .get_aws_secrets_manager_client(aws_cfg).await?;

        if let Some(monitoring) = &ctx.deployment_config.monitoring {
            let logs_secret_id = Self::otlp_auth_secret_name_for(ctx.resource_prefix);
            Self::aws_upsert_secret(
                secrets_client.as_ref(),
                &logs_secret_id,
                &monitoring.logs_auth_header,
                "Alien OTLP logs auth header for horizond agents",
                &config,
            )
            .await?;
            self.otlp_auth_secret_name = Some(logs_secret_id);

            if let Some(metrics_auth_header) = &monitoring.metrics_auth_header {
                let metrics_secret_id =
                    Self::otlp_metrics_auth_secret_name_for(ctx.resource_prefix);
                Self::aws_upsert_secret(
                    secrets_client.as_ref(),
                    &metrics_secret_id,
                    metrics_auth_header,
                    "Alien OTLP metrics auth header for horizond agents",
                    &config,
                )
                .await?;
                self.otlp_metrics_auth_secret_name = Some(metrics_secret_id);
            } else {
                self.otlp_metrics_auth_secret_name = None;
            }

            info!("OTLP secrets updated in Secrets Manager");
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
        Ok(HandlerAction::Continue {
            state: ResizingExistingAsgs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = ResizingExistingAsgs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn resizing_existing_asgs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;

        for group in &config.capacity_groups {
            if let Some(state) = self.asg_states.get_mut(&group.group_id) {
                if let Some(asg_name) = state.asg_name.clone() {
                    asg_client
                        .update_auto_scaling_group(
                            UpdateAutoScalingGroupRequest::builder()
                                .auto_scaling_group_name(asg_name.clone())
                                .min_size(group.min_size as i32)
                                .max_size(group.max_size as i32)
                                .build(),
                        )
                        .await
                        .context(ErrorData::CloudPlatformError {
                            message: format!("Failed to update ASG {}", asg_name),
                            resource_id: Some(config.id.clone()),
                        })?;
                    state.desired_size = group.min_size;
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingRemovedAsgs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingRemovedAsgs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn deleting_removed_asgs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let removed_group_ids: Vec<String> = self
            .asg_states
            .keys()
            .filter(|id| !config.capacity_groups.iter().any(|g| &g.group_id == *id))
            .cloned()
            .collect();

        for group_id in &removed_group_ids {
            self.delete_capacity_group(ctx, group_id, &config.id)
                .await?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingNewLaunchTemplates,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNewLaunchTemplates,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_launch_templates(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let new_groups: Vec<CapacityGroup> = config
            .capacity_groups
            .iter()
            .filter(|g| !self.asg_states.contains_key(&g.group_id))
            .cloned()
            .collect();

        for group in &new_groups {
            if self.launch_templates.contains_key(&group.group_id) {
                continue;
            }
            info!(group_id = %group.group_id, "Creating launch template for new capacity group");
            self.create_launch_template_for_group(ctx, group, &config)
                .await?;
        }

        Ok(HandlerAction::Continue {
            state: CreatingNewAsgs,
            suggested_delay: None,
        })
    }

    #[handler(
        state = CreatingNewAsgs,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_asgs(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        let new_groups: Vec<CapacityGroup> = config
            .capacity_groups
            .iter()
            .filter(|g| !self.asg_states.contains_key(&g.group_id))
            .cloned()
            .collect();

        for group in &new_groups {
            info!(group_id = %group.group_id, "Creating ASG for new capacity group");
            self.create_asg_for_group(ctx, group, &config).await?;
        }

        self.new_groups_pending_ready = new_groups.iter().map(|g| g.group_id.clone()).collect();
        Ok(HandlerAction::Continue {
            state: WaitingForNewGroupsReady,
            suggested_delay: if self.new_groups_pending_ready.is_empty() {
                None
            } else {
                Some(Duration::from_secs(30))
            },
        })
    }

    #[handler(
        state = WaitingForNewGroupsReady,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn waiting_for_new_groups_ready(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if self.new_groups_pending_ready.is_empty() {
            return Ok(HandlerAction::Continue {
                state: CreatingNewLaunchTemplateVersion,
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
                state: CreatingNewLaunchTemplateVersion,
                suggested_delay: None,
            });
        }

        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;
        let new_asg_names: Vec<String> = self
            .new_groups_pending_ready
            .iter()
            .filter_map(|id| self.asg_states.get(id)?.asg_name.clone())
            .collect();
        if new_asg_names.is_empty() {
            self.new_groups_pending_ready.clear();
            return Ok(HandlerAction::Continue {
                state: CreatingNewLaunchTemplateVersion,
                suggested_delay: None,
            });
        }

        let desc = asg_client
            .describe_auto_scaling_groups(
                DescribeAutoScalingGroupsRequest::builder()
                    .auto_scaling_group_names(new_asg_names)
                    .build(),
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to describe new ASGs".to_string(),
                resource_id: Some(config.id.clone()),
            })?;

        let groups = desc
            .describe_auto_scaling_groups_result
            .auto_scaling_groups
            .map(|w| w.members)
            .unwrap_or_default();
        let mut all_healthy = true;
        let mut total = 0u32;
        for asg in groups {
            let asg_name = asg.auto_scaling_group_name.unwrap_or_default();
            let instances = asg.instances.map(|w| w.members).unwrap_or_default();
            let healthy = instances
                .iter()
                .filter(|i| i.health_status.as_deref() == Some("Healthy"))
                .count();
            let desired = asg.desired_capacity.unwrap_or(0) as usize;
            if healthy < desired {
                all_healthy = false;
            }
            total += healthy as u32;
            if let Some(state) = self
                .asg_states
                .values_mut()
                .find(|s| s.asg_name.as_deref() == Some(&asg_name))
            {
                state.current_size = healthy as u32;
            }
        }
        if all_healthy && total > 0 {
            info!(total, "All new ASG instances healthy");
            self.new_groups_pending_ready.clear();
            return Ok(HandlerAction::Continue {
                state: CreatingNewLaunchTemplateVersion,
                suggested_delay: None,
            });
        }

        self.boot_check_iterations += 1;
        if self.boot_check_iterations >= BOOT_DIAG_TIMEOUT_ITERATIONS {
            let new_states: HashMap<String, AsgState> = self
                .asg_states
                .iter()
                .filter(|(id, _)| self.new_groups_pending_ready.contains(id))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let boot_log = Self::collect_console_output(
                &*ctx.service_provider.get_aws_ec2_client(aws_cfg).await?,
                &new_states,
            )
            .await;
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "New ASG instances not healthy after ~15 min.\n{}",
                    super::summarize_boot_log(&boot_log.unwrap_or_default())
                ),
                resource_id: Some(config.id.clone()),
            }));
        }

        Ok(HandlerAction::Continue {
            state: WaitingForNewGroupsReady,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    // ─────────────── ROLLING UPDATE STATES ─────────────────────

    #[handler(
        state = CreatingNewLaunchTemplateVersion,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating,
    )]
    async fn creating_new_launch_template_version(
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

        self.rolling_update_poll_iterations = 0;

        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
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
        let otlp_auth = self.otlp_auth_secret_name.clone();

        for (group_id, lt_state) in &self.launch_templates {
            let group_id_str = group_id.as_str();
            let machine_token_secret =
                Self::machine_token_secret_name(ctx.resource_prefix, &config.id);
            let user_data = self.generate_user_data(
                &cluster_id,
                &api_url,
                &template_inputs.horizond_download_base_url,
                group_id_str,
                &machine_token_secret,
                template_inputs.monitoring_logs_endpoint.as_deref(),
                template_inputs.monitoring_metrics_endpoint.as_deref(),
                otlp_auth.as_deref(),
                self.otlp_metrics_auth_secret_name.as_deref(),
            );

            info!(template_id = %lt_state.template_id, group_id = %group_id, "Creating new launch template version");

            ec2_client
                .create_launch_template_version(
                    CreateLaunchTemplateVersionRequest::builder()
                        .launch_template_id(lt_state.template_id.clone())
                        .source_version("$Latest".to_string())
                        .version_description("rolling-update".to_string())
                        .launch_template_data(
                            RequestLaunchTemplateData::builder()
                                .user_data(user_data)
                                .build(),
                        )
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to create new LT version for group '{}'", group_id),
                    resource_id: Some(config.id.clone()),
                })?;
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
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;

        self.instance_refresh_ids.clear();

        for (group_id, asg_state) in &self.asg_states {
            if let Some(asg_name) = &asg_state.asg_name {
                info!(asg_name = %asg_name, group_id = %group_id, "Starting instance refresh");

                let response = asg_client
                    .start_instance_refresh(
                        StartInstanceRefreshRequest::builder()
                            .auto_scaling_group_name(asg_name.clone())
                            .strategy("Rolling".to_string())
                            .preferences(
                                RefreshPreferences::builder()
                                    .min_healthy_percentage(100)
                                    .max_healthy_percentage(110)
                                    .build(),
                            )
                            .build(),
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to start instance refresh for ASG {}", asg_name),
                        resource_id: Some(config.id.clone()),
                    })?;

                let refresh_id = response
                    .start_instance_refresh_result
                    .instance_refresh_id
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::CloudPlatformError {
                            message: format!(
                                "StartInstanceRefresh for ASG {} returned no refresh ID",
                                asg_name
                            ),
                            resource_id: Some(config.id.clone()),
                        })
                    })?;
                self.instance_refresh_ids
                    .insert(asg_name.clone(), refresh_id);
            }
        }

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
                state: Ready,
                suggested_delay: None,
            });
        }

        let config = ctx.desired_resource_config::<ContainerCluster>()?;
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;

        const ROLLING_UPDATE_TIMEOUT: u32 = 60;

        let mut all_complete = true;

        for (asg_name, refresh_id) in &self.instance_refresh_ids {
            let response = asg_client
                .describe_instance_refreshes(
                    DescribeInstanceRefreshesRequest::builder()
                        .auto_scaling_group_name(asg_name.clone())
                        .instance_refresh_ids(vec![refresh_id.clone()])
                        .build(),
                )
                .await
                .context(ErrorData::CloudPlatformError {
                    message: format!("Failed to describe instance refresh for ASG {}", asg_name),
                    resource_id: Some(config.id.clone()),
                })?;

            let refresh = response
                .describe_instance_refreshes_result
                .instance_refreshes
                .and_then(|w| w.members.into_iter().next());

            match refresh.as_ref().and_then(|r| r.status.as_deref()) {
                Some("Successful") => {
                    debug!(asg_name = %asg_name, "Instance refresh completed");
                }
                Some("Failed") | Some("Cancelled") => {
                    let reason = refresh
                        .as_ref()
                        .and_then(|r| r.status_reason.clone())
                        .unwrap_or_default();
                    return Err(AlienError::new(ErrorData::CloudPlatformError {
                        message: format!(
                            "Instance refresh for ASG {} failed: {}",
                            asg_name, reason
                        ),
                        resource_id: Some(config.id.clone()),
                    }));
                }
                Some(status) => {
                    debug!(asg_name = %asg_name, status = %status, "Instance refresh in progress");
                    all_complete = false;
                }
                None => {
                    all_complete = false;
                }
            }
        }

        if all_complete {
            info!("All instance refreshes complete");
            self.rolling_update_triggered = false;
            self.instance_refresh_ids.clear();
            return Ok(HandlerAction::Continue {
                state: Ready,
                suggested_delay: None,
            });
        }

        self.rolling_update_poll_iterations += 1;
        if self.rolling_update_poll_iterations >= ROLLING_UPDATE_TIMEOUT {
            let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;
            let boot_log = Self::collect_console_output(&*ec2_client, &self.asg_states).await;
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

    // ─────────────── DELETE FLOW ──────────────────────────────

    #[flow_entry(Delete)]
    #[handler(
        state = DeleteStart,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<ContainerCluster>()?;

        info!(cluster_id = %config.id, "Starting ContainerCluster deletion");

        Ok(HandlerAction::Continue {
            state: DeletingAutoScalingGroups,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingAutoScalingGroups,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_auto_scaling_groups(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let asg_client = ctx.service_provider.get_aws_autoscaling_client(aws_cfg).await?;

        for state in self.asg_states.values() {
            if let Some(asg_name) = &state.asg_name {
                info!(asg_name = %asg_name, "Deleting Auto Scaling Group");

                match asg_client
                    .delete_auto_scaling_group(
                        DeleteAutoScalingGroupRequest::builder()
                            .auto_scaling_group_name(asg_name.clone())
                            .force_delete(true)
                            .build(),
                    )
                    .await
                {
                    Ok(_) => info!(asg_name = %asg_name, "ASG deleted"),
                    Err(e)
                        if matches!(
                            e.error,
                            Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        ) =>
                    {
                        warn!(asg_name = %asg_name, "ASG already deleted");
                    }
                    Err(e) => {
                        warn!(asg_name = %asg_name, error = ?e, "Failed to delete ASG, continuing");
                    }
                }
            }
        }

        self.asg_states.clear();

        Ok(HandlerAction::Continue {
            state: DeletingHealthCheckTargetGroup,
            suggested_delay: Some(Duration::from_secs(5)),
        })
    }

    #[handler(
        state = DeletingHealthCheckTargetGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_health_check_target_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        if let Some(tg_arn) = &self.target_group_arn {
            let aws_cfg = ctx.get_aws_config()?;
            let elbv2_client = ctx.service_provider.get_aws_elbv2_client(aws_cfg).await?;

            info!(target_group_arn = %tg_arn, "Deleting health check target group");

            match elbv2_client.delete_target_group(tg_arn).await {
                Ok(_) => info!(target_group_arn = %tg_arn, "Target group deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    info!(target_group_arn = %tg_arn, "Target group already deleted")
                }
                Err(e) => {
                    warn!(target_group_arn = %tg_arn, error = ?e, "Failed to delete target group, continuing");
                }
            }
        }

        self.target_group_arn = None;

        Ok(HandlerAction::Continue {
            state: DeletingLaunchTemplate,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingLaunchTemplate,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_launch_template(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        for template in self.launch_templates.values() {
            let template_id = &template.template_id;
            info!(template_id = %template_id, "Deleting launch template");

            match ec2_client
                .delete_launch_template(
                    DeleteLaunchTemplateRequest::builder()
                        .launch_template_id(template_id.clone())
                        .build(),
                )
                .await
            {
                Ok(_) => info!(template_id = %template_id, "Launch template deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(template_id = %template_id, "Launch template already deleted");
                }
                Err(e) => {
                    warn!(template_id = %template_id, error = ?e, "Failed to delete launch template, continuing");
                }
            }
        }

        self.launch_templates.clear();

        // Delete OTLP auth header secrets if they were created
        let secrets_client = ctx
            .service_provider
            .get_aws_secrets_manager_client(aws_cfg).await?;
        for otlp_secret_name in [
            self.otlp_auth_secret_name.take(),
            self.otlp_metrics_auth_secret_name.take(),
        ]
        .into_iter()
        .flatten()
        {
            info!(secret_name = %otlp_secret_name, "Deleting OTLP secret");
            match secrets_client
                .delete_secret(
                    DeleteSecretRequest::builder()
                        .secret_id(otlp_secret_name.clone())
                        .force_delete_without_recovery(true)
                        .build(),
                )
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
                    warn!(secret_name = %otlp_secret_name, error = ?e, "Failed to delete OTLP secret, continuing");
                }
            }
        }

        Ok(HandlerAction::Continue {
            state: DeletingSecurityGroup,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingSecurityGroup,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_security_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg).await?;

        if let Some(sg_id) = &self.security_group_id {
            info!(sg_id = %sg_id, "Deleting security group");

            match ec2_client.delete_security_group(sg_id).await {
                Ok(_) => info!(sg_id = %sg_id, "Security group deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(sg_id = %sg_id, "Security group already deleted");
                }
                Err(e) => {
                    warn!(sg_id = %sg_id, error = ?e, "Failed to delete security group, continuing");
                }
            }

            self.security_group_id = None;
        }

        Ok(HandlerAction::Continue {
            state: DeletingIamResources,
            suggested_delay: None,
        })
    }

    #[handler(
        state = DeletingIamResources,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting,
    )]
    async fn deleting_iam_resources(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let aws_cfg = ctx.get_aws_config()?;
        let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg).await?;

        // Remove role from instance profile
        if let (Some(profile_name), Some(role_name)) =
            (&self.instance_profile_name, &self.role_name)
        {
            info!(profile_name = %profile_name, role_name = %role_name, "Removing role from instance profile");

            match iam_client
                .remove_role_from_instance_profile(profile_name, role_name)
                .await
            {
                Ok(_) => info!("Role removed from instance profile"),
                Err(e) => {
                    warn!(error = ?e, "Failed to remove role from instance profile, continuing")
                }
            }
        }

        // Delete instance profile
        if let Some(profile_name) = &self.instance_profile_name {
            info!(profile_name = %profile_name, "Deleting instance profile");

            match iam_client.delete_instance_profile(profile_name).await {
                Ok(_) => info!(profile_name = %profile_name, "Instance profile deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(profile_name = %profile_name, "Instance profile already deleted");
                }
                Err(e) => {
                    warn!(profile_name = %profile_name, error = ?e, "Failed to delete instance profile, continuing");
                }
            }

            self.instance_profile_name = None;
            self.instance_profile_arn = None;
        }

        // Detach policies from role
        if let Some(role_name) = &self.role_name {
            let policies = vec![
                "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore",
                "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy",
            ];

            for policy_arn in policies {
                match iam_client.detach_role_policy(role_name, policy_arn).await {
                    Ok(_) => debug!(policy = %policy_arn, "Policy detached"),
                    Err(e) => {
                        warn!(policy = %policy_arn, error = ?e, "Failed to detach policy, continuing")
                    }
                }
            }

            // Delete role
            info!(role_name = %role_name, "Deleting IAM role");

            match iam_client.delete_role(role_name).await {
                Ok(_) => info!(role_name = %role_name, "IAM role deleted"),
                Err(e)
                    if matches!(
                        e.error,
                        Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    warn!(role_name = %role_name, "IAM role already deleted");
                }
                Err(e) => {
                    warn!(role_name = %role_name, error = ?e, "Failed to delete IAM role, continuing");
                }
            }

            self.role_name = None;
            self.role_arn = None;
        }

        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
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

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        let cluster_id = self.horizon_cluster_id.as_ref()?;

        let capacity_group_statuses: Vec<_> = self
            .asg_states
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
            horizon_ready: true,
            capacity_group_statuses,
            total_machines,
        }))
    }

    fn get_binding_params(&self) -> Option<serde_json::Value> {
        None
    }
}

impl AwsContainerClusterController {
    /// Creates a controller in ready state with mock values for testing.
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(cluster_id: &str, capacity_groups: Vec<(&str, u32)>) -> Self {
        let mut asg_states = HashMap::new();
        for (group_id, size) in capacity_groups {
            asg_states.insert(
                group_id.to_string(),
                AsgState {
                    group_id: group_id.to_string(),
                    asg_name: Some(format!("mock-{}-{}", cluster_id, group_id)),
                    current_size: size,
                    desired_size: size,
                    instance_type: Some("m7g.medium".to_string()),
                },
            );
        }

        Self {
            state: AwsContainerClusterState::Ready,
            role_name: Some(format!("{}-role", cluster_id)),
            role_arn: Some(format!(
                "arn:aws:iam::123456789012:role/{}-role",
                cluster_id
            )),
            instance_profile_name: Some(format!("{}-profile", cluster_id)),
            instance_profile_arn: Some(format!(
                "arn:aws:iam::123456789012:instance-profile/{}-profile",
                cluster_id
            )),
            launch_templates: HashMap::from([(
                "general".to_string(),
                LaunchTemplateState {
                    group_id: "general".to_string(),
                    template_id: format!("lt-{}", cluster_id),
                    template_name: format!("{}-lt", cluster_id),
                },
            )]),
            security_group_id: Some(format!("sg-{}", cluster_id)),
            target_group_arn: Some(format!("arn:aws:elasticloadbalancing:us-east-1:123456789012:targetgroup/{}-hc-tg/1234567890", cluster_id)),
            asg_states,
            horizon_cluster_id: Some(cluster_id.to_string()),
            horizon_api_url: Some("https://horizon.example.com".to_string()),
            boot_check_iterations: 0,
            rolling_update_poll_iterations: 0,
            otlp_auth_secret_name: None,
            otlp_metrics_auth_secret_name: None,
            new_groups_pending_ready: vec![],
            rolling_update_triggered: false,
            instance_refresh_ids: HashMap::new(),
            _internal_stay_count: None,
        }
    }
}
