# Network

VPC/VNet infrastructure for Containers that need internal networking. Network is auto-generated from stack settings—developers don't define it directly.

## When Networks Are Created

Networks are only needed when the stack contains Containers. Function-only stacks don't require network infrastructure.

```typescript
// No network needed - serverless
const func = new alien.Function("api").code(...).build()

// Network required - containers need internal networking
const db = new alien.Container("postgres").code({ type: "image", image: "postgres:16" }).build()
```

## Configuration

Network settings live in `StackSettings`, not as a resource in `alien.ts`:

```json
{
  "network": {
    "type": "create",
    "availabilityZones": 2,
    "natGateway": true
  }
}
```

### Modes

**No Network**: Stack has no network infrastructure. Valid when stack only contains Functions, Storage, KV, Queue.

**Create**: Alien creates VPC/VNet with subnets, NAT gateways, etc.

**BYO-VPC**: Use existing infrastructure. For enterprises with pre-approved network architectures.

## Stack Settings Schema

```rust
pub enum NetworkSettings {
    Create {
        cidr: Option<String>,         // Auto-generated if not specified
        availability_zones: u8,        // Default: 2
        nat_gateway: bool,             // Default: true
    },
    ByoVpcAws {
        vpc_id: String,
        public_subnet_ids: Vec<String>,
        private_subnet_ids: Vec<String>,
        security_group_ids: Vec<String>,
    },
    ByoVpcGcp {
        network_name: String,
        subnet_name: String,
        region: String,
    },
    ByoVnetAzure {
        vnet_resource_id: String,
        public_subnet_name: String,
        private_subnet_name: String,
    },
}
```

## CIDR Auto-Generation

When CIDR isn't specified, the controller queries existing VPCs and finds an available range.

Priority order:
1. `100.64-127.0.0/16` (RFC 6598) — Rarely conflicts with on-premises networks
2. `172.16-31.0.0/16` — Less common than 10.x
3. `10.0-255.0.0/16` — Last resort (commonly used everywhere)

```rust
async fn find_available_cidr(&self) -> Result<String> {
    let used_cidrs = self.get_existing_vpc_cidrs().await?;
    
    // Start with hash-based offset for determinism
    let stack_hash = self.stack_id.bytes().fold(0u8, |acc, b| acc.wrapping_add(b)) % 64;
    
    // Try 100.64.x first
    for attempt in 0..64 {
        let octet = 64 + ((stack_hash + attempt) % 64);
        let candidate = format!("100.{}.0.0/16", octet);
        if !overlaps_any(&candidate, &used_cidrs) {
            return Ok(candidate);
        }
    }
    // Fall back to 172.16.x, then 10.x...
}
```

**When to specify CIDR explicitly:**
- VPC peering with known CIDRs
- Transit Gateway connections
- VPN to on-premises

## Public Subnets

Public subnets are automatically included when:
- Any Function has `ingress: "public"`
- Any Container has `.expose({ port: 80 })`

This is checked at preflight time. For BYO-VPC, preflight validates that `public_subnet_ids` is non-empty if public access is needed.

## Preflights

### Compile-Time Checks (No Cloud Access)

| Check | Purpose |
|-------|---------|
| `NetworkRequiredCheck` | Network configured when Container/ContainerCluster exists |
| `PublicSubnetsRequiredCheck` | Public subnets configured when public ingress exists |
| `NetworkSettingsPlatformCheck` | BYO settings match target platform |

### Runtime Checks (Before Controllers)

| Check | Purpose |
|-------|---------|
| `ByoVpcExistsCheck` | VPC/VNet actually exists |
| `ByoSubnetsExistCheck` | All subnet IDs exist |
| `ByoSubnetsInVpcCheck` | Subnets belong to the specified VPC |
| `ByoSecurityGroupsExistCheck` | Security groups exist |

### BYO-VPC Validation (AWS)

| Check | API Call |
|-------|----------|
| VPC exists | `ec2:DescribeVpcs` |
| VPC in correct region | `ec2:DescribeVpcs` |
| DNS support enabled | `ec2:DescribeVpcAttribute` |
| DNS hostnames enabled | `ec2:DescribeVpcAttribute` |
| All subnets exist | `ec2:DescribeSubnets` |
| Subnets in VPC | `ec2:DescribeSubnets` |
| Subnets span multiple AZs | `ec2:DescribeSubnets` |
| Public subnets route to IGW | `ec2:DescribeRouteTables` |
| Private subnets route to NAT | `ec2:DescribeRouteTables` |
| Security groups exist | `ec2:DescribeSecurityGroups` |
| Security groups in VPC | `ec2:DescribeSecurityGroups` |

### BYO-VPC Validation (GCP)

| Check | API Call |
|-------|----------|
| VPC network exists | `compute.networks.get` |
| Subnet exists | `compute.subnetworks.get` |
| Subnet in correct region | `compute.subnetworks.get` |
| Private Google Access | `compute.subnetworks.get` |
| Cloud NAT configured | `compute.routers.list` |

### BYO-VNet Validation (Azure)

| Check | API Call |
|-------|----------|
| VNet exists | `virtualNetworks.get` |
| VNet in correct region | `virtualNetworks.get` |
| Subnets exist | `subnets.get` |
| NAT Gateway attached | `natGateways.get` |

## Controller Behavior

### Create Mode

1. Create VPC/VNet
2. Create Internet Gateway (if public subnets needed)
3. Create public subnets (if needed)
4. Create private subnets
5. Create NAT Gateway (if configured)
6. Create route tables
7. Create security groups
8. Transition to Ready

### BYO-VPC Mode

1. Populate controller state from settings (VPC ID, subnet IDs)
2. Transition to Ready immediately

Runtime preflights have already validated the infrastructure exists.

## Resource Dependencies

Resources that need network details access them via `require_dependency`:

```rust
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<ContainerCluster>()?;
    
    // Get Network controller state
    let network_state = ctx.require_dependency::<AwsNetworkController>(&config.network)?;
    
    // Access platform-specific fields
    let vpc_id = network_state.vpc_id.as_ref().ok_or(...)?;
    let private_subnet_ids = &network_state.private_subnet_ids;
    
    // Create instances in the VPC...
}
```

## Platform Controller State

### AWS

```rust
pub struct AwsNetworkController {
    pub vpc_id: Option<String>,
    pub internet_gateway_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub public_subnet_ids: Vec<String>,
    pub private_subnet_ids: Vec<String>,
    pub public_route_table_id: Option<String>,
    pub private_route_table_id: Option<String>,
    pub security_group_id: Option<String>,
}
```

### GCP

```rust
pub struct GcpNetworkController {
    pub network_name: Option<String>,
    pub network_self_link: Option<String>,
    pub subnet_name: Option<String>,
    pub subnet_self_link: Option<String>,
    pub cloud_nat_name: Option<String>,
    pub router_name: Option<String>,
}
```

### Azure

```rust
pub struct AzureNetworkController {
    pub vnet_id: Option<String>,
    pub vnet_name: Option<String>,
    pub public_subnet_id: Option<String>,
    pub private_subnet_id: Option<String>,
    pub nat_gateway_id: Option<String>,
    pub nsg_id: Option<String>,
}
```

### Kubernetes/Local

No Network controller. Preflights return `should_run: false` for these platforms.

## CloudFormation Support

### Template Generation

```rust
impl CloudFormationGenerator for NetworkCloudFormationGenerator {
    fn generate_cloudformation_resources(&self, resource: &Resource, ...) -> Result<()> {
        match &network.settings {
            NetworkSettings::Create { cidr, availability_zones, nat_gateway } => {
                // Generate: VPC, Subnets, IGW, NAT, Route Tables, Security Groups
                self.generate_create_resources(...)?;
            }
            NetworkSettings::ByoVpcAws { vpc_id, public_subnet_ids, private_subnet_ids, security_group_ids } => {
                // No resources created, but outputs for other resources
                self.generate_byo_outputs(...)?;
            }
            _ => return Err(Error::InvalidPlatform),
        }
        Ok(())
    }
}
```

### State Import

Importing existing VPC state from CloudFormation:

```rust
impl CloudFormationImporter for NetworkCloudFormationImporter {
    async fn import_cloudformation_state(&self, resource: &Resource, context: &CloudFormationImportContext) -> Result<Box<dyn ResourceController>> {
        match &network.settings {
            NetworkSettings::ByoVpcAws { vpc_id, public_subnet_ids, private_subnet_ids, security_group_ids } => {
                // Use settings directly
                AwsNetworkController {
                    vpc_id: Some(vpc_id.clone()),
                    public_subnet_ids: public_subnet_ids.clone(),
                    private_subnet_ids: private_subnet_ids.clone(),
                    security_group_id: security_group_ids.first().cloned(),
                    state: AwsNetworkState::Ready,
                }
            }
            NetworkSettings::Create { .. } => {
                // Import from CloudFormation physical IDs
                let vpc_id = context.cfn_resources.get(&vpc_logical_id)?;
                // ... import subnet IDs, etc.
            }
        }
    }
}
```

## Outputs

Network outputs are cloud-agnostic for observability:

```rust
pub struct NetworkOutputs {
    pub network_id: String,           // Human-readable identifier
    pub availability_zones: u8,       // Number of AZs
    pub has_public_subnets: bool,
    pub has_nat_gateway: bool,
    pub cidr: Option<String>,         // If created by Alien
}
```

Platform-specific details (VPC IDs, subnet ARNs) come from controller state, not outputs.

## Examples

### Create New Network

```json
{
  "network": {
    "type": "create",
    "availabilityZones": 3,
    "natGateway": true
  }
}
```

### Create with Explicit CIDR (for Peering)

```json
{
  "network": {
    "type": "create",
    "cidr": "10.50.0.0/16",
    "availabilityZones": 3,
    "natGateway": true
  }
}
```

### BYO-VPC (AWS)

```json
{
  "network": {
    "type": "byo-vpc-aws",
    "vpcId": "vpc-0123456789abcdef0",
    "publicSubnetIds": ["subnet-pub-1a", "subnet-pub-1b"],
    "privateSubnetIds": ["subnet-priv-1a", "subnet-priv-1b"],
    "securityGroupIds": ["sg-0123456789abcdef0"]
  }
}
```


