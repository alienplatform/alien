# Containers - Deployment Flow

Step-by-step walkthrough of deploying a Container, showing how Alien and Horizon work together.

## Resource Model

Alien Containers use two resource types:

**ContainerCluster:**
- Manages compute infrastructure (multiple ASGs per capacity group, IAM roles)
- Creates Horizon cluster with capacity group definitions
- Scales ASGs based on Horizon's capacity plan

**Container:**
- Manages containerized workload
- Assigns to capacity group (general, storage, gpu, etc.)
- Registers with Horizon
- Creates load balancers (if exposed)
- Provisions volumes (if stateful)

**Note:** A single ContainerCluster can have multiple capacity groups (e.g., general + storage + GPU), each backed by its own ASG. All machines join the same Horizon cluster for unified networking and scheduling.

## Horizon Configuration

Horizon configuration is prepared by the platform before deployment and passed via `DeploymentConfig`.

### Platform Preparation (Before Deployment)

The platform ensures Horizon clusters exist and prepares secure token distribution:

```typescript
// In platform when building DeploymentConfig
// This runs BEFORE deployment starts

async function buildDeploymentConfig(params: {
  db: Kysely<Database>
  deployment: Deployment
  manager: Manager | undefined
  stack: Stack
  // ...
}): Promise<DeploymentConfig> {
  // ... other config ...

  // Find all ContainerCluster resources in stack
  const containerClusters = Object.entries(stack.resources)
    .filter(([_, r]) => r.config.resourceType() === "container-cluster");

  let horizonConfig = null;
  if (containerClusters.length > 0) {
    const clusterConfigs: Record<string, HorizonClusterConfig> = {};
    const builtInEnvVars: EnvironmentVariable[] = [];

    // Ensure each cluster exists in Horizon
    for (const [resourceId, _] of containerClusters) {
      const clusterId = `${deployment.workspaceId}/${deployment.projectId}/${deployment.id}/${resourceId}`;

      // Check if cluster already exists in deployment.horizonConfig
      let storedCluster = deployment.horizonConfig?.clusters?.[resourceId];

      if (!storedCluster) {
        // Create cluster in Horizon
        const platformJwt = await generatePlatformJwt({ scopes: ["cluster:*"] });
        const response = await horizonClient.createCluster({
          clusterId,
          name: `${params.deployment.projectId}-${params.deployment.id}-${resourceId}`,
          containerCidr: "10.244.0.0/16",
          platform: deployment.platform,
        }, {
          headers: { Authorization: `Bearer ${platformJwt}` }
        });

        // Tokens returned ONCE ONLY - encrypt and store in platform DB
        const project = await db.selectFrom("projects")
          .select("envEncryptionKey")
          .where("id", "=", deployment.projectId)
          .executeTakeFirstOrThrow();

        // Update deployment.horizonConfig with encrypted tokens
        const updatedHorizonConfig = deployment.horizonConfig || { clusters: {} };
        updatedHorizonConfig.clusters[resourceId] = {
          clusterId,
          managementToken: encryptEnvironmentVariableValue(response.managementToken, project.envEncryptionKey),
          machineToken: encryptEnvironmentVariableValue(response.machineToken, project.envEncryptionKey),
          createdAt: new Date().toISOString(),
        };

        await db.updateTable("deployments")
          .set({ horizonConfig: updatedHorizonConfig })
          .where("id", "=", deployment.id)
          .execute();

        storedCluster = updatedHorizonConfig.clusters[resourceId];
      }

      // Decrypt tokens for deployment
      const project = await getProject(db, deployment.projectId);
      const managementToken = decryptEnvironmentVariableValue(storedCluster.managementToken, project.envEncryptionKey);
      const machineToken = decryptEnvironmentVariableValue(storedCluster.machineToken, project.envEncryptionKey);

      // Add management token to DeploymentConfig (for API calls)
      clusterConfigs[resourceId] = {
        clusterId,
        managementToken,  // For alien-deployment API calls
      };

      // Add machine token as built-in environment variable
      // This will be synced to vault (Parameter Store/Secret Manager/Key Vault)
      builtInEnvVars.push({
        name: `ALIEN_HORIZON_MACHINE_TOKEN_${resourceId.toUpperCase().replace(/-/g, '_')}`,
        value: machineToken,
        type: "secret",  // Synced to vault during Provisioning phase
        targetFunctions: null,
      });
    }
    
    horizonConfig = {
      url: process.env.HORIZON_URL || "https://horizon.alien.dev",
      clusters: clusterConfigs,
    };
  }
  
  // Merge built-in env vars into snapshot
  const envVarsSnapshot = await getEnvironmentVariablesSnapshot({
    db,
    deployment,
    manager,
    stack,
    // Built-in vars (ALIEN_DEPLOYMENT_ID, OTEL_*, ALIEN_HORIZON_MACHINE_TOKEN_*) added here
  });
  
  return {
    managementConfig: ...,
    stackSettings: ...,  // Includes deploymentModel, network, approvals
    environmentVariables: envVarsSnapshot,  // Includes machine tokens
    horizonConfig,  // Cluster IDs + management tokens for API calls
  };
}
```

**How machine tokens flow:**
1. Platform decrypts token from `deployment.horizonConfig`
2. Adds as built-in env var with resource-specific name:
   - Pattern: `ALIEN_HORIZON_MACHINE_TOKEN_{RESOURCE_ID}`
   - Example: ContainerCluster `id: "compute"` → `ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE`
   - Example: ContainerCluster `id: "gpu-pool"` → `ALIEN_HORIZON_MACHINE_TOKEN_GPU_POOL`
3. Included in environment variables snapshot
4. During Provisioning: `sync_secrets_to_vault()` writes to Secrets Manager/Secret Manager/Key Vault
5. User data fetches token at boot using IAM role/service account

### DeploymentConfig Structure

```rust
// alien-core/src/deployment.rs

/// Configuration for a single Horizon cluster
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonClusterConfig {
    /// Cluster ID (deterministic: workspace/project/deployment/resourceid)
    pub cluster_id: String,
    
    /// Management token for API access (hm_...)
    /// Used by alien-deployment controllers to create/update containers
    pub management_token: String,
    
    // Note: Machine token (hj_...) is NOT in DeploymentConfig
    // It's added to environmentVariables snapshot as a built-in secret variable
    // and synced to vault (Parameter Store/Secret Manager/Key Vault)
}

/// Horizon configuration for container orchestration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct HorizonConfig {
    /// Horizon API base URL
    pub url: String,
    
    /// Cluster configurations (one per ContainerCluster resource)
    /// Key: ContainerCluster resource ID from stack
    /// Value: Cluster ID and management token for that cluster
    pub clusters: HashMap<String, HorizonClusterConfig>,
}

pub struct DeploymentConfig {
    pub management_config: Option<ManagementConfig>,
    pub stack_settings: StackSettings,  // deploymentModel, network, approvals
    pub environment_variables: EnvironmentVariablesSnapshot,
    
    /// Horizon configuration (cluster IDs and management tokens)
    /// Generated by platform, includes management tokens for API calls
    /// Machine tokens are in environmentVariables as built-in secret vars
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizon_config: Option<HorizonConfig>,
}
```

**Token management:**
- **Management tokens** (`hm_...`): In `horizon_config` for API calls
- **Machine tokens** (`hj_...`): In `environment_variables` as built-in secret vars
  - Synced to vault during Provisioning phase
  - Fetched by VMs from Parameter Store/Secret Manager/Key Vault at boot

**Cluster ID format:** `{workspace_id}/{project_id}/{deployment_id}/{resource_id}` (deterministic, stable across retries)

**Token storage in platform DB:**
- Encrypted in `deployment.horizon_config` JSONB field
- One record per ContainerCluster resource
- Decrypted only when building DeploymentConfig

### Passing to Controllers

```rust
// alien-infra/src/core/controller.rs
pub struct ResourceControllerContext<'a> {
    // ... existing fields ...
    
    /// Horizon configuration (passed from deployment config)
    pub horizon_config: Option<&'a HorizonConfig>,
}

// In alien-deployment when calling controllers:
let controller_context = ResourceControllerContext {
    // ... other fields ...
    horizon_config: config.horizon_config.as_ref(),
};
```

**Usage in ContainerClusterController:**
```rust
// Note: Cluster already created by platform before deployment
// Controller just provisions infrastructure (ASG, IAM roles, etc.)

async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    let horizon_config = ctx.horizon_config.unwrap();
    
    // Look up cluster config (cluster already exists)
    let cluster_config = horizon_config.clusters.get(&config.id)
        .ok_or_else(|| AlienError::new(ErrorData::InfrastructureError {
            message: format!("No Horizon cluster config found for resource '{}'", config.id),
            operation: Some("create_start".to_string()),
            resource_id: Some(config.id.clone()),
        }))?;
    
    // Verify cluster exists in Horizon
    let horizon_client = HorizonClient::new(&horizon_config.url);
    horizon_client.get_cluster(
        &cluster_config.cluster_id,
        &cluster_config.management_token  // Use management token for API calls
    ).await?;
    
    self.horizon_cluster_id = Some(cluster_config.cluster_id.clone());
    
    // Continue to provision infrastructure (IAM role, launch template, ASG)
    Ok(HandlerAction::Continue {
        state: CreateIamRole,
        suggested_delay: None,
    })
}
```

**Usage in ContainerController:**
```rust
async fn create_horizon_container(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    
    // Get ContainerCluster resource ID from config
    let cluster_resource_id = &config.cluster.id;
    
    let horizon_config = ctx.horizon_config.unwrap();
    let cluster_config = horizon_config.clusters.get(cluster_resource_id).unwrap();
    
    // Use cluster ID and management token for API calls
    let horizon_client = HorizonClient::new(&horizon_config.url);
    horizon_client.create_container(
        &cluster_config.cluster_id,
        &cluster_config.management_token,  // Management token for container operations
        request
    ).await?;
}
```

## ContainerClusterController

Manages infrastructure and Horizon cluster setup.

### Create Flow

```
CreateStart → CreateIamRole → CreateLaunchTemplate → CreateAsg → 
CreateHorizonCluster → Ready
```

#### CreateStart: Create IAM Role

```rust
#[flow_entry(Create)]
#[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let aws_cfg = ctx.get_aws_config()?;
    let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg)?;
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    
    let role_name = format!("{}-{}-instance-role", ctx.resource_prefix, config.id);
    
    // Create IAM role for EC2 instances
    let assume_role_policy = json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": "ec2.amazonaws.com" },
            "Action": "sts:AssumeRole"
        }]
    });
    
    iam_client.create_role(
        &role_name,
        &serde_json::to_string(&assume_role_policy)?,
        Some("Alien ContainerCluster instance role"),
    ).await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create IAM role".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    
    // Attach policies (ECR pull)
    iam_client.attach_role_policy(
        &role_name,
        "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
    ).await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to attach IAM policy".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    
    self.iam_role_name = Some(role_name);
    
    Ok(HandlerAction::Continue {
        state: CreateLaunchTemplate,
        suggested_delay: Some(Duration::from_secs(5)),
    })
}
```

#### CreateIamRole → CreateLaunchTemplate

```rust
#[handler(state = CreateLaunchTemplate, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_launch_template(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let aws_cfg = ctx.get_aws_config()?;
    let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
    let iam_client = ctx.service_provider.get_aws_iam_client(aws_cfg)?;
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    
    let template_name = format!("{}-{}-lt", ctx.resource_prefix, config.id);
    let profile_name = format!("{}-{}-profile", ctx.resource_prefix, config.id);
    
    // Create instance profile
    iam_client.create_instance_profile(&profile_name).await?;
    iam_client.add_role_to_instance_profile(
        &profile_name,
        self.iam_role_name.as_ref().unwrap()
    ).await?;
    
    // User data script (will be updated in CreateHorizonCluster with actual credentials)
    let user_data = generate_horizond_user_data_template()?;
    
    // Create launch template
    let template = ec2_client.create_launch_template(CreateLaunchTemplateRequest {
        name: template_name.clone(),
        instance_type: config.instance_type.clone(),
        image_id: get_ami_for_instance_type(&config.instance_type, &aws_cfg.region)?,
        iam_instance_profile: profile_name,
        user_data,
        // ... other config
    }).await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create launch template".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    
    self.launch_template_id = Some(template.launch_template_id);
    
    Ok(HandlerAction::Continue {
        state: CreateAsg,
        suggested_delay: Some(Duration::from_secs(5)),
    })
}
```

#### CreateLaunchTemplate → CreateAsg

```rust
#[handler(state = CreateAsg, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_asg(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let aws_cfg = ctx.get_aws_config()?;
    let asg_client = ctx.service_provider.get_aws_asg_client(aws_cfg)?;
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    
    let asg_name = format!("{}-{}-asg", ctx.resource_prefix, config.id);
    let vpc_subnets = get_vpc_subnets(ctx)?;
    
    // Create Auto Scaling Group
    asg_client.create_auto_scaling_group(CreateAutoScalingGroupRequest {
        name: asg_name.clone(),
        launch_template_id: self.launch_template_id.clone().unwrap(),
        min_size: config.min_size,
        max_size: config.max_size,
        desired_capacity: config.min_size,
        vpc_zone_identifier: vpc_subnets.join(","),
        health_check_type: "EC2".to_string(),
        health_check_grace_period: 300,
        tags: vec![
            Tag { key: "alien:cluster".into(), value: config.id.clone() },
        ],
    }).await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to create Auto Scaling Group".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    
    self.asg_name = Some(asg_name);
    
    Ok(HandlerAction::Continue {
        state: CreateHorizonCluster,
        suggested_delay: Some(Duration::from_secs(10)),
    })
}
```

#### CreateAsg → Ready

Note: There is **no** `CreateHorizonCluster` state anymore. The cluster is already created by the platform before deployment starts. The controller only provisions infrastructure.

**User data script generation:**

```rust
fn generate_horizond_user_data(
    cluster_id: &str,
    horizon_api_url: &str,
    vault_prefix: &str,          // e.g., "myapp-prod-secrets"
    machine_token_key: &str,     // e.g., "ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE"
) -> Result<String> {
    Ok(format!(r#"#!/bin/bash
set -euo pipefail

INSTANCE_ID=$(ec2-metadata --instance-id | cut -d " " -f 2)
ZONE=$(ec2-metadata --availability-zone | cut -d " " -f 2)
REGION=$(echo $ZONE | sed 's/[a-z]$//')

# Calculate container CIDR suffix (unique per machine)
CONTAINER_CIDR_SUFFIX=$(($(echo $INSTANCE_ID | md5sum | cut -c1-2 | awk '{{print "0x"$1}}') % 254 + 1))

# Fetch machine token from Secrets Manager (SECURE - using IAM role)
# Alien vault naming: {{vault_prefix}}-{{machine_token_key}}
# Example: myapp-prod-secrets-ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE
MACHINE_TOKEN=$(aws secretsmanager get-secret-value \
  --secret-id {{vault_prefix}}-{{machine_token_key}} \
  --region $REGION \
  --query SecretString \
  --output text)

# Install dependencies
apt-get update && apt-get install -y containerd wireguard-tools nftables awscli

# Install CNI plugins
CNI_VERSION="v1.4.0"
mkdir -p /opt/cni/bin
curl -sSL "https://github.com/containernetworking/plugins/releases/download/${{CNI_VERSION}}/cni-plugins-linux-arm64-${{CNI_VERSION}}.tgz" \
  | tar -xz -C /opt/cni/bin

# Install horizond
curl -L https://releases.horizon.dev/horizond-latest-linux-arm64 -o /usr/local/bin/horizond
chmod +x /usr/local/bin/horizond

# Start horizond (token in memory, never written to disk)
cat > /etc/systemd/system/horizond.service <<'SVCEOF'
[Unit]
Description=Horizon Machine Agent
After=network.target containerd.service
Requires=containerd.service

[Service]
Environment="CLUSTER_ID={cluster_id}"
Environment="MACHINE_TOKEN=$MACHINE_TOKEN"
Environment="API_URL={horizon_api_url}"
Environment="MACHINE_ID=$INSTANCE_ID"
Environment="CONTAINER_CIDR=10.244.$CONTAINER_CIDR_SUFFIX.0/24"
Environment="CONTAINER_GATEWAY=10.244.$CONTAINER_CIDR_SUFFIX.1"
ExecStart=/usr/local/bin/horizond
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
SVCEOF

systemctl enable horizond
systemctl start horizond
"#,
        cluster_id = cluster_id,
        horizon_api_url = horizon_api_url,
        vault_prefix = vault_prefix,
        machine_token_key = machine_token_key,
    ))
}
```

**Security improvements:**
- ✅ Token fetched from Secrets Manager (not hardcoded in user data)
- ✅ Access controlled via IAM role (only this agent's instances)
- ✅ Token encrypted with KMS in Secrets Manager
- ✅ Not visible in launch template or EC2 console
- ✅ Audit trail via CloudTrail

**What this creates:**
- IAM instance role (ECR pull + Secrets Manager read access)
- Launch template with secure horizond user data (fetches token from vault)
- Auto Scaling Group (min/max from config)

#### Ready: Machine Autoscaling

```rust
#[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    let horizon_config = ctx.horizon_config.unwrap();
    
    // Look up cluster config for this resource
    let cluster_config = horizon_config.clusters.get(&config.id).unwrap();
    
    // Query Horizon for capacity plan (using management token)
    let horizon_client = HorizonClient::new(&horizon_config.url);
    let capacity_plan = horizon_client.get_capacity_plan(
        &cluster_config.cluster_id,
        &cluster_config.management_token,
    ).await?;
    
    // Apply Horizon's decisions to each ASG
    let aws_cfg = ctx.get_aws_config()?;
    let asg_client = ctx.service_provider.get_aws_asg_client(aws_cfg)?;
    
    for group_plan in capacity_plan.groups {
        let asg_name = self.asg_names.get(&group_plan.group_id)
            .ok_or_else(|| anyhow!("No ASG found for group {}", group_plan.group_id))?;
        
        if group_plan.desired_machines != group_plan.current_machines {
            asg_client.set_desired_capacity(
                asg_name,
                group_plan.desired_machines
            ).await?;
            
            info!(
                group = %group_plan.group_id,
                from = group_plan.current_machines,
                to = group_plan.desired_machines,
                reason = %group_plan.reason,
                "Scaled ASG based on Horizon capacity plan"
            );
        }
    }
    
    Ok(HandlerAction::Stay {
        suggested_delay: Some(Duration::from_secs(60)),
    })
}
```

### Controller State

```rust
use alien_macros::controller;

#[controller]
pub struct AwsContainerClusterController {
    /// IAM role name for EC2 instances
    iam_role_name: Option<String>,
    
    /// Launch template ID
    launch_template_id: Option<String>,
    
    /// Auto Scaling Group name
    asg_name: Option<String>,
    
    /// Horizon cluster ID
    horizon_cluster_id: Option<String>,
    
    // Note: Tokens NOT stored in controller state
    // - Management token: passed via DeploymentConfig (ephemeral)
    // - Machine token: synced to vault, fetched by VMs at boot
}

impl AwsContainerClusterController {
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        if let (Some(cluster_id), Some(asg_name)) = (
            &self.horizon_cluster_id,
            &self.asg_name,
        ) {
            Some(ResourceOutputs::new(ContainerClusterOutputs {
                asg_name: asg_name.clone(),
                horizon_cluster_id: cluster_id.clone(),
            }))
        } else {
            None
        }
    }
}
```

**Note:** Tokens are **not stored** in controller state:
- **Management token**: Available via `ctx.horizon_config` (ephemeral, from DeploymentConfig)
- **Machine token**: Synced to vault as built-in environment variable, fetched by VMs at boot

## ContainerController

Manages containerized workloads through Horizon.

### Create Flow

```
CreateStart → ProvisionVolumes → CreateLoadBalancer → CreateHorizonContainer → Ready
```

**Order matters:**
1. **ProvisionVolumes** - Provision EBS volumes (if stateful with persistent storage)
2. **CreateLoadBalancer** - Create load balancer + target groups (if exposed)
3. **CreateHorizonContainer** - Call Horizon API with volumes and loadBalancerTarget

This ensures Alien has provisioned all infrastructure before calling Horizon.

#### CreateStart: Validate Image

```rust
#[flow_entry(Create)]
#[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    
    // Containers only support pre-built images (like Functions)
    let image_uri = match &config.code {
        ContainerCode::Image { image } => image.clone(),
        ContainerCode::Source { .. } => {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                message: "Container is configured with source code, but only pre-built images are supported at runtime".to_string(),
                resource_id: Some(config.id.clone()),
            }));
        }
    };
    
    self.image_uri = Some(image_uri);
    
    Ok(HandlerAction::Continue {
        state: CreateHorizonContainer,
        suggested_delay: None,
    })
}
```

**Note:** Image building happens during `alien build` (before deployment). The controller just validates that an image URI is available.

#### CreateStart → CreateHorizonContainer

**Note:** This happens AFTER CreateLoadBalancer so we have the LoadBalancerTarget to pass to Horizon.

```rust
#[handler(state = CreateHorizonContainer, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_horizon_container(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    let horizon_config = ctx.horizon_config.unwrap();
    
    // Get ContainerCluster resource ID from Container config
    let cluster_resource_id = &config.cluster.id;
    
    // Look up cluster config from Horizon configuration
    let cluster_config = horizon_config.clusters.get(cluster_resource_id)
        .ok_or_else(|| AlienError::new(ErrorData::DependencyNotFound {
            resource_id: config.id.clone(),
            dependency_id: cluster_resource_id.to_string(),
        }))?;
    
    // Build environment variables (includes links, secrets, etc.)
    let env_vars = EnvironmentVariableBuilder::new(&config.environment)
        .add_standard_alien_env_vars(ctx)
        .add_linked_resources(&config.links, ctx, &config.id)
        .await?
        .build();
    
    // Build load balancer target (if created in CreateLoadBalancer)
    let load_balancer_target = if !self.target_groups.is_empty() {
        Some(match ctx.platform {
            Platform::Aws => LoadBalancerTarget::Aws {
                target_group_arn: self.target_groups[0].arn.clone(),
            },
            Platform::Gcp => LoadBalancerTarget::Gcp {
                backend_service_url: self.target_groups[0].arn.clone(),  // Stored in same field
            },
            Platform::Azure => LoadBalancerTarget::Azure {
                backend_pool_id: self.target_groups[0].arn.clone(),
                resource_group: ctx.resource_group.clone(),
            },
        })
    } else {
        None
    };
    
    // Create container in Horizon (using JWT)
    let horizon_client = HorizonClient::new(&horizon_config.url);
    
    let container_request = CreateContainerRequest {
        name: config.id.clone(),
        capacity_group: config.capacity_group.clone(),  // Auto-assigned by preflights
        image: self.image_uri.clone().unwrap(),
        
        resources: ResourceRequirements {
            cpu: ResourceSpec {
                min: config.cpu.to_string(),
                desired: config.cpu.to_string(),
            },
            memory: ResourceSpec {
                min: config.memory.clone(),
                desired: config.memory.clone(),
            },
            ephemeral_storage: config.ephemeral_storage.clone(),
            gpu: config.gpu.as_ref().map(|gpu| GpuSpec {
                r#type: gpu.gpu_type.clone(),
                count: gpu.count,
            }),
        },
        
        autoscaling: if !config.stateful {
            Some(AutoscalingConfig {
                min: config.min_replicas.unwrap_or(1),
                desired: config.min_replicas.unwrap_or(1),
                max: config.max_replicas.unwrap_or(10),
                target_cpu_percent: 70.0,
                target_memory_percent: 80.0,
            })
        } else {
            None
        },
        
        replicas: if config.stateful {
            Some(config.replicas.unwrap_or(1))
        } else {
            None
        },
        
        stateful: config.stateful,
        ports: config.ports.clone(),
        load_balancer_target,  // Platform-specific enum (optional)
        env: env_vars,
        
        // Include volumes if stateful with persistent storage (as platform-specific enums)
        volumes: if !self.volumes.is_empty() {
            Some(self.volumes.clone())  // Vec<VolumeRegistration> with VolumeTarget enums
        } else {
            None
        },
    };
    
    horizon_client.create_container(
        &cluster_config.cluster_id,
        &cluster_config.jwt,
        container_request
    ).await
        .context(ErrorData::HorizonApiError {
            message: "Failed to create container in Horizon".to_string(),
            operation: "create_container".to_string(),
            resource_id: config.id.clone(),
        })?;
    
    self.horizon_container_name = Some(config.id.clone());
    
    info!(container = %config.id, "Created container in Horizon");
    
    Ok(HandlerAction::Continue {
        state: Ready,
        suggested_delay: Some(Duration::from_secs(5)),
    })
}
```

#### CreateStart → ProvisionVolumes (If Stateful)

**Note:** This step provisions block volumes (EBS, Persistent Disks, Managed Disks) before creating the container in Horizon.

```rust
#[handler(state = ProvisionVolumes, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn provision_volumes(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    
    // Skip if not stateful or no persistent storage declared
    if !config.stateful || config.persistent_storage.is_none() {
        return Ok(HandlerAction::Continue {
            state: CreateLoadBalancer,
            suggested_delay: None,
        });
    }
    
    // Validate: stateful must have fixed replicas
    if config.replicas.is_none() {
        return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
            message: "Stateful containers with persistent storage must specify fixed replicas count".to_string(),
            resource_id: Some(config.id.clone()),
        }));
    }
    
    let zones = get_availability_zones(ctx)?;
    let replica_count = config.replicas.unwrap();
    let size_gb = parse_storage_gb(config.persistent_storage.as_ref().unwrap());
    
    let mut volumes = Vec::new();
    for ordinal in 0..replica_count {
        let zone = &zones[ordinal as usize % zones.len()];
        
        // Create volume using platform-specific API
        let volume_target = match ctx.platform {
            Platform::Aws => {
                let aws_cfg = ctx.get_aws_config()?;
                let ec2_client = ctx.service_provider.get_aws_ec2_client(aws_cfg)?;
                
                let volume = ec2_client.create_volume(CreateVolumeRequest {
                    size: size_gb,
                    availability_zone: zone.clone(),
                    volume_type: "gp3".to_string(),
                    iops: Some(3000),
                    throughput: Some(125),
                    tags: vec![
                        Tag { key: "alien:container".into(), value: config.id.clone() },
                        Tag { key: "alien:ordinal".into(), value: ordinal.to_string() },
                    ],
                }).await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create EBS volume for ordinal {}", ordinal),
                        resource_id: Some(config.id.clone()),
                    })?;
                
                VolumeTarget::Aws {
                    volume_id: volume.volume_id,
                    zone: zone.clone(),
                }
            }
            Platform::Gcp => {
                let gcp_cfg = ctx.get_gcp_config()?;
                let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_cfg)?;
                
                let disk_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, ordinal);
                compute_client.create_disk(CreateDiskRequest {
                    name: disk_name.clone(),
                    size_gb,
                    zone: zone.clone(),
                    disk_type: "pd-ssd".to_string(),
                }).await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create GCP disk for ordinal {}", ordinal),
                        resource_id: Some(config.id.clone()),
                    })?;
                
                VolumeTarget::Gcp {
                    disk_name,
                    zone: zone.clone(),
                }
            }
            Platform::Azure => {
                let azure_cfg = ctx.get_azure_config()?;
                let compute_client = ctx.service_provider.get_azure_compute_client(azure_cfg)?;
                
                let disk_name = format!("{}-{}-{}", ctx.resource_prefix, config.id, ordinal);
                let disk = compute_client.create_managed_disk(CreateDiskRequest {
                    name: disk_name,
                    size_gb,
                    location: zone.clone(),
                    sku: "Premium_LRS".to_string(),
                    resource_group: ctx.resource_group.clone(),
                }).await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create Azure disk for ordinal {}", ordinal),
                        resource_id: Some(config.id.clone()),
                    })?;
                
                VolumeTarget::Azure {
                    disk_id: disk.id,
                    zone: zone.clone(),
                    resource_group: ctx.resource_group.clone(),
                }
            }
        };
        
        volumes.push(VolumeRegistration {
            ordinal,
            volume: volume_target,
        });
    }
    
    self.volumes = volumes;
    
    info!(container = %config.id, count = self.volumes.len(), "Created persistent volumes");
    
    Ok(HandlerAction::Continue {
        state: CreateLoadBalancer,
        suggested_delay: None,
    })
}
```

#### ProvisionVolumes → CreateLoadBalancer

#### CreateLoadBalancer → CreateHorizonContainer

```rust
#[handler(state = CreateLoadBalancer, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
async fn create_load_balancer(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    
    // Skip if not exposed
    if config.expose.is_empty() {
        return Ok(HandlerAction::Continue {
            state: CreateHorizonContainer,
            suggested_delay: None,
        });
    }
    
    let aws_cfg = ctx.get_aws_config()?;
    let elb_client = ctx.service_provider.get_aws_elb_client(aws_cfg)?;
    
    // Determine LB type
    let lb_type = if config.expose.iter().all(|e| e.protocol == "http" || e.protocol == "https") {
        "application"  // ALB
    } else {
        "network"      // NLB
    };
    
    let lb_name = format!("{}-{}-lb", ctx.resource_prefix, config.id);
    
    // Create load balancer
    let lb = elb_client.create_load_balancer(CreateLoadBalancerRequest {
        name: lb_name,
        lb_type: lb_type.to_string(),
        scheme: "internet-facing".to_string(),
        subnets: get_public_subnets(ctx)?,
        security_groups: if lb_type == "application" {
            Some(vec![self.create_lb_security_group(ctx).await?])
        } else {
            None
        },
        tags: vec![
            Tag { key: "alien:container".into(), value: config.id.clone() },
        ],
    }).await?;
    
    self.load_balancer_arn = Some(lb.load_balancer_arn.clone());
    self.load_balancer_dns = Some(lb.dns_name.clone());
    
    // Create target groups + listeners for each exposed port
    for expose_config in &config.expose {
        let target_port = expose_config.target_port
            .unwrap_or_else(|| config.ports.first().cloned().unwrap_or(80));
        
        let tg = elb_client.create_target_group(CreateTargetGroupRequest {
            name: format!("{}-{}-tg-{}", ctx.resource_prefix, config.id, expose_config.port),
            protocol: if expose_config.protocol == "http" { "HTTP" } else { "TCP" }.to_string(),
            port: target_port,
            vpc_id: get_vpc_id(ctx)?,
            target_type: "instance".to_string(),
            health_check: HealthCheckConfig {
                protocol: if expose_config.protocol == "http" { "HTTP" } else { "TCP" }.to_string(),
                port: "traffic-port".to_string(),
                path: if expose_config.protocol == "http" { Some("/health".to_string()) } else { None },
                interval_seconds: 10,
                timeout_seconds: 5,
                healthy_threshold_count: 2,
                unhealthy_threshold_count: 2,
            },
            deregistration_delay_seconds: 30,
        }).await?;
        
        // Create listener
        elb_client.create_listener(CreateListenerRequest {
            load_balancer_arn: lb.load_balancer_arn.clone(),
            protocol: if expose_config.https { "HTTPS" } else { "HTTP" }.to_string(),
            port: expose_config.port,
            default_actions: vec![
                Action::Forward {
                    target_group_arn: tg.target_group_arn.clone(),
                }
            ],
            certificates: if expose_config.https {
                Some(vec![self.get_or_create_certificate(ctx, expose_config).await?])
            } else {
                None
            },
        }).await?;
        
        self.target_groups.push(TargetGroupInfo {
            port: expose_config.port,
            target_port,
            arn: tg.target_group_arn,  // AWS: target group ARN, GCP: backend service URL, Azure: backend pool ID
        });
    }
    
    info!(
        container = %config.id,
        lb_dns = %self.load_balancer_dns.as_ref().unwrap(),
        "Created load balancer"
    );
    
    Ok(HandlerAction::Continue {
        state: CreateHorizonContainer,
        suggested_delay: None,
    })
}
```

#### Ready: Monitor Container Health

```rust
#[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Container>()?;
    let horizon_config = ctx.horizon_config.unwrap();
    
    // Get ContainerCluster resource ID from Container config
    let cluster_resource_id = &config.cluster.id;
    
    // Look up cluster config from Horizon configuration
    let cluster_config = horizon_config.clusters.get(cluster_resource_id)
        .ok_or_else(|| AlienError::new(ErrorData::DependencyNotFound {
            resource_id: config.id.clone(),
            dependency_id: cluster_resource_id.to_string(),
        }))?;
    
    let horizon_client = HorizonClient::new(&horizon_config.url);
    
    // Poll container status from Horizon (using management token)
    let container = horizon_client.get_container(
        &cluster_config.cluster_id,
        &config.id,
        &cluster_config.management_token,
    ).await
        .context(ErrorData::HorizonApiError {
            message: "Failed to get container status".to_string(),
            operation: "get_container".to_string(),
            resource_id: config.id.clone(),
        })?;
    
    // Check container status (similar to Lambda's Active state check)
    match container.status.as_str() {
        "running" => {
            // Container is healthy and serving traffic
            debug!(
                container = %config.id,
                replicas = container.replicas.len(),
                "Container health check passed"
            );
            
            Ok(HandlerAction::Stay {
                suggested_delay: Some(Duration::from_secs(60)),
            })
        }
        "pending" => {
            // Container is still starting up - keep polling
            debug!(
                container = %config.id,
                "Container still pending, waiting for replicas to start"
            );
            
            Ok(HandlerAction::Stay {
                max_times: 20,  // Allow up to 20 retries (~2 minutes for containers to start)
                suggested_delay: Some(Duration::from_secs(5)),
            })
        }
        "failed" => {
            // Non-retriable failure
            Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Container is in failed state".to_string(),
            }))
        }
        "stopped" => {
            // Resource drift - container was stopped
            Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Container is stopped, expected running".to_string(),
            }))
        }
        other => {
            Err(AlienError::new(ErrorData::HorizonApiError {
                message: format!("Unexpected container status: {}", other),
                operation: "get_container".to_string(),
                resource_id: config.id.clone(),
            }))
        }
    }
}
```

**What this does:**
- Polls Horizon API every 60 seconds during normal operation (heartbeat check)
- During initial creation: polls every 5 seconds (max 20 times) until status becomes "running"
- Fails fast on "failed" or "stopped" status (resource drift, non-retriable)
- Similar pattern to Lambda's `CreateWaitForActive` state
- **No LB target syncing** - horizond handles that automatically at runtime

### Controller State

```rust
#[controller]
pub struct AwsContainerController {
    /// Container image URI
    image_uri: Option<String>,
    
    /// Horizon container name (same as config.id)
    horizon_container_name: Option<String>,
    
    /// Load balancer ARN (if exposed)
    load_balancer_arn: Option<String>,
    
    /// Load balancer DNS name (if exposed)
    load_balancer_dns: Option<String>,
    
    /// Target groups (one per exposed port)
    /// Note: arn field stores platform-specific identifier (target group ARN, backend service URL, etc.)
    target_groups: Vec<TargetGroupInfo>,
    
    /// Volumes (for stateful containers) - platform-specific
    volumes: Vec<VolumeRegistration>,
}

struct VolumeRegistration {
    ordinal: u32,
    volume: VolumeTarget,  // Platform-specific enum
}

impl AwsContainerController {
    fn build_outputs(&self) -> Option<ResourceOutputs> {
        self.horizon_container_name.as_ref().map(|name| {
            ResourceOutputs::new(ContainerOutputs {
                container_name: name.clone(),
                image_uri: self.image_uri.clone(),
                load_balancer_dns: self.load_balancer_dns.clone(),
                volumes: self.volumes.clone(),
            })
        })
    }
}
```

## Data Flow Summary

**Platform Preparation (Before Deployment):**
```
Platform checks deployment.horizonConfig for existing clusters
  → If new: Platform calls Horizon API to create cluster (Platform JWT auth)
  → Platform encrypts tokens and stores in deployment.horizonConfig (platform DB)
  → Platform adds machine token as built-in environment variable
  → Platform includes management token in DeploymentConfig
```

**InitialSetup (ContainerCluster):**
```
Alien creates secrets vault (if not exists)
  → Alien creates IAM role (with Parameter Store read access)
  → Alien creates launch template (user data fetches token from vault)
  → Alien creates ASG
  → Machines boot with horizond
```

**Provisioning:**
```
Alien syncs secrets to vault (including machine token)
  → Machine token now in Parameter Store/Secret Manager/Key Vault
  
For each Container:
  → Alien validates image URI
  → Alien provisions EBS volumes (if stateful with persistent storage)
  → Alien creates load balancer + target groups (if exposed)
  → Alien calls Horizon API to create container:
      * With management token auth
      * Includes volumes array (VolumeTarget enums)
      * Includes loadBalancerTarget (LoadBalancerTarget enum)
  → Horizon scheduler assigns replicas to machines (zone-aware for stateful)
  → horizond starts containers, attaches volumes, registers LB targets
```

**Running (Both in Ready state):**
```
ContainerCluster Ready (every 60s):
  → Query Horizon for capacity plan (with management token)
  → Scale ASGs per group if needed

Container Ready (every 60s):
  → Query Horizon for container status (with management token)
  → Check container health
  → (No LB sync - horizond handles that)

horizond (every 5s):
  → Heartbeat to Horizon (with machine token from vault)
  → Receive assignments
  → Start/stop containers
  → Attach/detach volumes (if stateful)
  → Register/deregister LB targets (if exposed)
```

## Integration Points

**Platform → Horizon:**
- Create cluster: `POST /clusters` (Platform JWT auth)
  - Returns `managementToken` and `machineToken` (once only)
  - Platform stores encrypted in `deployment.horizonConfig`

**Alien → Horizon:**
- Create container: `POST /clusters/:id/containers` (management token auth)
- Update container: `PATCH /clusters/:id/containers/:name` (management token auth)
- Get cluster capacity: `GET /clusters/:id` (management token auth - for machine autoscaling)
- Get container status: `GET /clusters/:id/containers/:name` (management token auth - for health checks)

**Authentication:**
- **Platform JWT**: Used by platform to create/delete clusters
- **Management token**: Used by alien-deployment for all container operations
- **Machine token**: Used by horizond for heartbeat/metrics
  - Stored in vault (Parameter Store/Secret Manager/Key Vault)
  - Fetched by VMs at boot using IAM role/service account

**Horizon → Machines:**
- horizond polls: `POST /heartbeat` (with machine token)
- horizond reports metrics: `POST /metrics` (with machine token)

**Horizon → horizond:**
- Heartbeat response includes assignments and cluster state
- horizond starts/stops containers based on assignments
- horizond configures networking (WireGuard, DNS, TCP proxies)

## Security: Machine Token Flow

**1. Platform creates cluster (before deployment):**
```typescript
const response = await horizonClient.createCluster({...});
// response.machineToken = "hj_abc123..."
```

**2. Platform encrypts and stores in database:**
```typescript
deployment.horizonConfig = {
  clusters: {
    "compute": {
      clusterId: "ws/proj/agent/compute",
      managementToken: encryptEnvironmentVariableValue(response.managementToken, projectKey),
      machineToken: encryptEnvironmentVariableValue(response.machineToken, projectKey),
    }
  }
};
```

**3. Platform adds machine token as built-in env var:**
```typescript
// In getEnvironmentVariablesSnapshotForAgent()
// Variable name is derived from ContainerCluster resource ID
// Example: resourceId="compute" → "ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE"
builtInVars.push({
  name: `ALIEN_HORIZON_MACHINE_TOKEN_${resourceId.toUpperCase().replace(/-/g, '_')}`,
  value: machineToken,  // Decrypted
  type: "secret",  // Will be synced to vault
  targetFunctions: null,
});
```

**4. During Provisioning: Token synced to vault:**
```rust
// sync_secrets_to_vault() writes to cloud storage
// Secret name pattern: {vaultPrefix}-ALIEN_HORIZON_MACHINE_TOKEN_{RESOURCE_ID}
// Example for ContainerCluster id="compute":
// - AWS: myapp-prod-secrets-ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE (Secrets Manager, $0.40/mo)
// - GCP: myapp-prod-secrets-ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE (Secret Manager, $0.06/mo)
// - Azure: myapp-prod-secrets/ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE (Key Vault, FREE)
```

**5. User data fetches token at boot:**
```bash
# Example for ContainerCluster id="compute"
MACHINE_TOKEN=$(aws secretsmanager get-secret-value \
  --secret-id myapp-prod-secrets-ALIEN_HORIZON_MACHINE_TOKEN_COMPUTE \
  --query SecretString --output text)

horizond --machine-token "$MACHINE_TOKEN"
```

**6. horizond uses token for all requests:**
```rust
http_client.post(format!("{}/heartbeat", api_url))
    .bearer_auth(&machine_token)
    .send()
    .await?;
```

## Next Steps

- **[Resource API](3-resource-api.md)** - Container configuration reference
- **[Infrastructure](4-infrastructure.md)** - How ContainerClusters work
- **[Machine Autoscaling](6-machine-autoscaling.md)** - ASG scaling logic
- **[Quickstart](2-quickstart.md)** - Deploy your first container

