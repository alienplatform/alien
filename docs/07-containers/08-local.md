# Containers - Local Platform

How to run Containers on the Local platform using Docker.

## Overview

The Local platform enables running Container resources on a single machine (Windows, Linux, macOS) using Docker. This is a **production-ready deployment target** for:

- **On-premise deployments** - Customer environments that don't allow cloud services
- **VM-based deployments** - "Just give me a VM" scenarios (EC2, GCE, Azure VM, VMware)
- **Edge devices** - Robots, IoT gateways, embedded Linux systems
- **Developer machines** - Local development and testing

The Local platform provides a simpler alternative to Horizon when you only need a single machine without orchestration complexity.

**Key differences from cloud:**
- **No Horizon:** Uses Docker directly instead of custom orchestration
- **Single machine:** All containers run on localhost
- **No machine autoscaling:** Fixed capacity (your local machine)
- **No load balancers:** Direct port mapping to localhost
- **Simplified networking:** Docker's built-in DNS

## Prerequisites

### Docker Installation

The Local platform requires Docker to be installed and running.

**Preflight Check:**

Add to `alien-preflights` (see [ALIEN_PREFLIGHTS.md](../ALIEN_PREFLIGHTS.md)):

```rust
/// Verify Docker is installed and accessible (Local platform only)
#[async_trait::async_trait]
pub struct DockerAvailableCheck;

impl CompileTimeCheck for DockerAvailableCheck {
    fn description(&self) -> &'static str {
        "Docker is installed and running"
    }
    
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Local && 
            stack.resources.values().any(|r| r.resource_type == "Container")
    }
    
    async fn check(&self, _stack: &Stack, _platform: Platform) -> Result<CheckResult> {
        // Try to connect to Docker daemon via bollard
        use bollard::Docker;
        
        match Docker::connect_with_local_defaults() {
            Ok(docker) => {
                // Try to ping Docker
                match docker.ping().await {
                    Ok(_) => CheckResult::success(),
                    Err(e) => CheckResult::error(format!(
                        "Docker is installed but not responding: {}. Try starting Docker Desktop.", 
                        e
                    )),
                }
            }
            Err(e) => CheckResult::error(format!(
                "Docker is not available: {}. Please install Docker Desktop.", 
                e
            )),
        }
    }
}
```

**Install Docker:**
- **macOS/Windows:** [Docker Desktop](https://www.docker.com/products/docker-desktop)
- **Linux:** Docker Engine via package manager

## Architecture

### Component Overview

**Docker Engine:**
- Single bridge network: `alien-network`
- All containers join this network
- DNS via network aliases: `postgres`, `postgres.svc`, `postgres-0.postgres.svc`
- Port mappings for exposed containers: `localhost:8080 → api:3000`

**LocalContainerManager** (via `bollard` crate):
- Create/manage containers
- Setup Docker network
- Configure DNS aliases
- Map ports for exposed containers

### State Management

Following the same pattern as local functions:

```
~/.alien-cli/{deployment_id}/
├── state.json                           # Deployment state
├── containers/
│   ├── postgres/
│   │   └── metadata.json                # Container metadata for recovery
│   ├── api/
│   │   └── metadata.json
│   └── worker/
│       └── metadata.json
└── volumes/
    └── postgres-0/                      # Persistent volumes (Docker volumes)
```

## Service Manager: LocalContainerManager

Similar to `LocalFunctionManager`, we need a `LocalContainerManager` that delegates to Docker via `bollard`.

### Manager Responsibilities

```rust
// alien-local/src/container_manager.rs

use bollard::Docker;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct LocalContainerManager {
    /// Docker client (via bollard)
    docker: Docker,
    
    /// Base directory for container state
    state_dir: PathBuf,
    
    /// Tracked containers (container_id → metadata)
    containers: Arc<Mutex<HashMap<String, ContainerMetadata>>>,
    
    /// Docker network name
    network_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMetadata {
    container_id: String,           // Service ID (e.g., "postgres")
    docker_container_id: String,    // Docker's internal ID
    image: String,
    ports: Vec<u16>,
    exposed_ports: HashMap<u16, u16>, // container_port → host_port
    stateful: bool,
    ordinal: Option<u32>,
    created_at: DateTime<Utc>,
}

impl LocalContainerManager {
    /// Create manager and setup Docker network
    pub fn new_with_shutdown(
        state_dir: PathBuf,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(Self, Option<JoinHandle<()>>)> {
        let docker = Docker::connect_with_local_defaults()
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to connect to Docker daemon".to_string(),
            })?;
        
        let network_name = "alien-network".to_string();
        
        let containers = Arc::new(Mutex::new(HashMap::new()));
        
        // Spawn background task for health monitoring and recovery
        let background_task = tokio::spawn({
            let state_dir = state_dir.clone();
            let containers = containers.clone();
            let docker = docker.clone();
            async move {
                Self::monitor_and_recover_loop(
                    state_dir,
                    containers,
                    docker,
                    shutdown_rx
                ).await;
            }
        });
        
        let manager = Self {
            docker,
            state_dir,
            containers,
            network_name,
        };
        
        Ok((manager, Some(background_task)))
    }
    
    /// Start a container
    pub async fn start_container(
        &self,
        container_id: &str,
        config: ContainerConfig,
    ) -> Result<ContainerInfo>;
    
    /// Stop a container
    pub async fn stop_container(&self, container_id: &str) -> Result<()>;
    
    /// Delete a container (stop + remove)
    pub async fn delete_container(&self, container_id: &str) -> Result<()>;
    
    /// Check if container is running
    pub async fn is_running(&self, container_id: &str) -> bool;
    
    /// Health check
    pub async fn check_health(&self, container_id: &str) -> Result<()>;
    
    /// Get binding for container
    pub async fn get_binding(&self, container_id: &str) -> Result<ContainerBinding>;
}
```

## Networking & Service Discovery

### Docker Network Setup

Create a single bridge network that all containers join:

```rust
impl LocalContainerManager {
    /// Ensure Docker network exists (idempotent)
    pub async fn ensure_network(&self) -> Result<()> {
        use bollard::network::{CreateNetworkOptions, InspectNetworkOptions};
        
        // Check if network exists
        match self.docker.inspect_network(
            &self.network_name, 
            None::<InspectNetworkOptions<String>>
        ).await {
            Ok(_) => {
                debug!("Docker network '{}' already exists", self.network_name);
                return Ok(());
            }
            Err(_) => {
                // Network doesn't exist, create it
            }
        }
        
        info!("Creating Docker network '{}'", self.network_name);
        
        let create_opts = CreateNetworkOptions {
            name: self.network_name.clone(),
            check_duplicate: true,
            driver: "bridge".to_string(),
            internal: false,
            attachable: true,
            ingress: false,
            ipam: Default::default(),
            enable_ipv6: false,
            options: HashMap::new(),
            labels: HashMap::new(),
        };
        
        self.docker.create_network(create_opts)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: "Failed to create Docker network".to_string(),
            })?;
        
        info!("✓ Docker network '{}' created", self.network_name);
        Ok(())
    }
}
```

### DNS with Network Aliases

Use Docker's built-in DNS with network aliases for `.svc` compatibility:

```rust
impl LocalContainerManager {
    pub async fn start_container(
        &self,
        container_id: &str,
        config: ContainerConfig,
    ) -> Result<ContainerInfo> {
        // Ensure network exists
        self.ensure_network().await?;
        
        // Build network aliases for DNS
        let mut network_aliases = vec![
            container_id.to_string(),           // e.g., "postgres"
            format!("{}.svc", container_id),    // e.g., "postgres.svc"
        ];
        
        // For stateful containers, add ordinal-specific aliases
        if config.stateful {
            let ordinal = config.ordinal.unwrap_or(0);
            network_aliases.push(format!(
                "{}-{}.{}.svc", 
                container_id, ordinal, container_id
            )); // e.g., "postgres-0.postgres.svc"
        }
        
        // Create container with network aliases
        let container_config = bollard::container::Config {
            image: Some(config.image.clone()),
            hostname: Some(container_id.to_string()),
            
            // Network configuration
            networking_config: Some(NetworkingConfig {
                endpoints_config: {
                    let mut map = HashMap::new();
                    map.insert(self.network_name.clone(), EndpointSettings {
                        aliases: Some(network_aliases),
                        ..Default::default()
                    });
                    map
                },
            }),
            
            // Environment variables
            env: Some(config.env_vars.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect()),
            
            // Port bindings (for exposed containers)
            exposed_ports: build_exposed_ports(&config),
            host_config: build_host_config(&config),
            
            ..Default::default()
        };
        
        // Create container
        let response = self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_id.to_string(),
                    platform: None,
                }),
                container_config,
            )
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to create container '{}'", container_id),
            })?;
        
        // Start container
        self.docker
            .start_container::<String>(
                &response.id,
                None,
            )
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to start container '{}'", container_id),
            })?;
        
        // Save metadata
        let metadata = ContainerMetadata {
            container_id: container_id.to_string(),
            docker_container_id: response.id.clone(),
            image: config.image,
            ports: config.ports,
            exposed_ports: config.exposed_ports.clone(),
            stateful: config.stateful,
            ordinal: config.ordinal,
            created_at: Utc::now(),
        };
        
        self.save_metadata(&metadata).await?;
        
        // Track in memory
        self.containers.lock().await.insert(
            container_id.to_string(),
            metadata.clone(),
        );
        
        Ok(ContainerInfo {
            container_id: container_id.to_string(),
            docker_container_id: response.id,
            exposed_ports: config.exposed_ports,
        })
    }
}
```

**DNS Resolution:**

```javascript
// Inside any container on alien-network:
const { Client } = require('pg');

// All of these work:
const client1 = new Client({ host: 'postgres' });      // Docker name
const client2 = new Client({ host: 'postgres.svc' }); // Horizon-compatible
const client3 = new Client({ host: 'postgres-0.postgres.svc' }); // Stateful
```

## Controller: LocalContainerController

Similar to `LocalFunctionController`, delegate to service manager:

```rust
// alien-infra/src/container/local.rs

use alien_macros::controller;

#[controller]
pub struct LocalContainerController {
    /// Docker container ID (service ID, not Docker's internal ID)
    container_id: Option<String>,
    
    /// Exposed ports mapping (container_port → host_port)
    exposed_ports: HashMap<u16, u16>,
}

impl LocalContainerController {
    // ─────────────── CREATE FLOW ───────────────────────────────────────────
    
    #[flow_entry(Create)]
    #[handler(
        state = StartingContainer,
        on_failure = ProvisionFailed,
        status = ResourceStatus::Provisioning
    )]
    async fn starting_container(
        &mut self, 
        ctx: &ResourceControllerContext<'_>
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        
        info!(container_id = %config.id, "Starting container");
        
        // Get container manager
        let container_mgr = ctx.service_provider.get_local_container_manager()
            .ok_or_else(|| AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "LocalContainerManager".to_string(),
            }))?;
        
        // Determine image
        let image = match &config.code {
            ContainerCode::Image { image } => image.clone(),
            ContainerCode::Source { .. } => {
                return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                    message: "Local platform does not support building from source yet".to_string(),
                    resource_id: Some(config.id.clone()),
                }));
            }
        };
        
        // Build container configuration
        let container_config = ContainerConfig {
            image,
            ports: config.ports.clone(),
            exposed_ports: allocate_host_ports(&config.expose)?,
            env_vars: config.environment.clone(),
            stateful: config.stateful,
            ordinal: config.ordinal,
            cpu: config.cpu,
            memory: config.memory.clone(),
            has_persistent_storage: config.persistent_storage.is_some(),
        };
        
        // Start container via manager
        let container_info = container_mgr
            .start_container(&config.id, container_config)
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to start container".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        
        self.container_id = Some(config.id.clone());
        self.exposed_ports = container_info.exposed_ports;
        
        info!(
            container_id = %config.id,
            exposed_ports = ?self.exposed_ports,
            "Container started successfully"
        );
        
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }
    
    #[handler(
        state = Ready,
        on_failure = RefreshFailed,
        status = ResourceStatus::Running
    )]
    async fn ready(
        &mut self, 
        ctx: &ResourceControllerContext<'_>
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        
        // Health check via manager
        let container_mgr = ctx.service_provider.get_local_container_manager()
            .ok_or_else(|| AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "LocalContainerManager".to_string(),
            }))?;
        
        container_mgr.check_health(&config.id)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Container health check failed for '{}'", config.id),
                resource_id: Some(config.id.clone()),
            })?;
        
        debug!(container_id = %config.id, "Container health check passed");
        
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }
    
    // ─────────────── UPDATE FLOW ──────────────────────────────────────────
    
    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(
        state = StoppingForUpdate,
        on_failure = UpdateFailed,
        status = ResourceStatus::Updating
    )]
    async fn stopping_for_update(
        &mut self, 
        ctx: &ResourceControllerContext<'_>
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let container_mgr = ctx.service_provider.get_local_container_manager()
            .ok_or_else(|| AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "LocalContainerManager".to_string(),
            }))?;
        
        info!(container_id = %config.id, "Stopping container for update");
        
        container_mgr.stop_container(&config.id).await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to stop container for update".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        
        Ok(HandlerAction::Continue {
            state: StartingContainer,
            suggested_delay: None,
        })
    }
    
    // ─────────────── DELETE FLOW ──────────────────────────────────────────
    
    #[flow_entry(Delete, from = [Ready, ProvisionFailed, RefreshFailed, UpdateFailed])]
    #[handler(
        state = Deleting,
        on_failure = DeleteFailed,
        status = ResourceStatus::Deleting
    )]
    async fn deleting(
        &mut self, 
        ctx: &ResourceControllerContext<'_>
    ) -> Result<HandlerAction> {
        let config = ctx.desired_resource_config::<Container>()?;
        let container_mgr = ctx.service_provider.get_local_container_manager()
            .ok_or_else(|| AlienError::new(ErrorData::LocalServicesNotAvailable {
                service_name: "LocalContainerManager".to_string(),
            }))?;
        
        info!(container_id = %config.id, "Deleting container");
        
        container_mgr.delete_container(&config.id).await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to delete container".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }
    
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    terminal_state!(state = ProvisionFailed, status = ResourceStatus::ProvisionFailed);
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(state = RefreshFailed, status = ResourceStatus::RefreshFailed);
    
    fn build_outputs(&self) -> Option<CoreResourceOutputs> {
        self.container_id.as_ref().map(|id| {
            CoreResourceOutputs::new(ContainerOutputs {
                container_name: id.clone(),
                image_uri: None,
                load_balancer_dns: None, // Local uses localhost:<port>
                exposed_urls: build_exposed_urls(&self.exposed_ports),
            })
        })
    }
}

/// Allocate host ports for exposed container ports
fn allocate_host_ports(expose_configs: &[ExposeConfig]) -> Result<HashMap<u16, u16>> {
    let mut mappings = HashMap::new();
    
    for expose in expose_configs {
        // Allocate a free port on localhost
        let host_port = port_check::free_local_port()
            .ok_or_else(|| AlienError::new(ErrorData::NoFreePorts {
                service_name: "container-expose".to_string(),
            }))?;
        
        mappings.insert(expose.port, host_port);
    }
    
    Ok(mappings)
}

/// Build exposed URLs for outputs
fn build_exposed_urls(port_mappings: &HashMap<u16, u16>) -> Vec<String> {
    port_mappings.iter()
        .map(|(_, host_port)| format!("http://localhost:{}", host_port))
        .collect()
}
```

## Port Mapping for Public Access

When a container has `.expose()`, map to localhost:

```rust
fn build_host_config(config: &ContainerConfig) -> Option<HostConfig> {
    if config.exposed_ports.is_empty() {
        return None;
    }
    
    // Build port bindings
    let mut port_bindings = HashMap::new();
    for (container_port, host_port) in &config.exposed_ports {
        port_bindings.insert(
            format!("{}/tcp", container_port),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".to_string()),
                host_port: Some(host_port.to_string()),
            }]),
        );
    }
    
    Some(HostConfig {
        port_bindings: Some(port_bindings),
        ..Default::default()
    })
}
```

**Result:**

```typescript
// alien.ts
const api = new alien.Container("api")
  .port(3000)
  .expose("http")
  .build()

// After deployment:
// - Container listens on port 3000 internally
// - Exposed at http://localhost:8080 (dynamically allocated)
```

## Example: Multi-Container App

```typescript
// alien.ts
const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi") // Docker volume
  .port(5432)
  .environment({
    POSTGRES_PASSWORD: "dev123",
    POSTGRES_DB: "myapp"
  })
  .build()

const api = new alien.Container("api")
  .code({ type: "image", image: "node:20-alpine" })
  .minReplicas(2)
  .maxReplicas(2) // Fixed on local (no autoscaling)
  .port(3000)
  .expose("http")
  .environment({
    DATABASE_URL: "postgresql://postgres:dev123@postgres.svc:5432/myapp"
  })
  .build()

export default new alien.Stack("local-app")
  .add(db, "live")
  .add(api, "live")
  .build()
```

**Deploy locally:**

```bash
alien run --name dev

# Output:
# ✅ Container postgres started
# ✅ Container api started (replica 1)
# ✅ Container api started (replica 2)
# 
# 🌐 Exposed services:
#   - api: http://localhost:8080
#
# Press Ctrl+C to stop
```

**What happens:**

1. Creates `alien-network` Docker bridge
2. Starts `postgres` container:
   - DNS: `postgres`, `postgres.svc`, `postgres-0.postgres.svc`
   - Docker volume for persistent storage
3. Starts `api` containers (2 replicas):
   - DNS: `api`, `api.svc`
   - Environment has `DATABASE_URL` with `postgres.svc`
   - Port 3000 (internal) → localhost:8080 (host)

**Test:**

```bash
# From host
curl http://localhost:8080/health

# From inside api container
docker exec -it api sh
> ping postgres.svc
# PING postgres.svc (172.18.0.2): Works!

> psql postgresql://postgres:dev123@postgres.svc:5432/myapp
# Connected!
```

## Persistent Storage

Use Docker volumes for persistent storage:

```rust
impl LocalContainerManager {
    async fn create_volume(&self, container_id: &str, ordinal: u32) -> Result<String> {
        use bollard::volume::CreateVolumeOptions;
        
        let volume_name = format!("{}-{}", container_id, ordinal);
        
        let create_opts = CreateVolumeOptions {
            name: volume_name.clone(),
            driver: "local".to_string(),
            driver_opts: HashMap::new(),
            labels: {
                let mut labels = HashMap::new();
                labels.insert("alien.container".to_string(), container_id.to_string());
                labels.insert("alien.ordinal".to_string(), ordinal.to_string());
                labels
            },
        };
        
        self.docker.create_volume(create_opts)
            .await
            .into_alien_error()
            .context(ErrorData::Other {
                message: format!("Failed to create volume '{}'", volume_name),
            })?;
        
        info!("✓ Created Docker volume '{}'", volume_name);
        Ok(volume_name)
    }
}
```

**Mount in container:**

```rust
host_config: Some(HostConfig {
    binds: Some(vec![
        format!("{}:/data", volume_name)
    ]),
    ..Default::default()
}),
```

## Limitations

### No Multi-Replica Autoscaling

Local platform runs on single machine - autoscaling doesn't make sense:

```typescript
// This works (fixed replicas)
.minReplicas(2).maxReplicas(2)

// This is simplified to fixed count on local
.minReplicas(2).maxReplicas(10)
// → Runs exactly minReplicas (2)
```

### No Load Balancers

Exposed containers map to localhost ports directly:

```typescript
// Cloud: Creates ALB at api-abc123.us-east-1.elb.amazonaws.com
// Local: Maps to http://localhost:8080

.expose("http")
```

### No Machine Autoscaling

Single machine = fixed capacity. No scaling like cloud deployments.

## Integration with `alien run`

From the CLI `run` command implementation:

```rust
// In run_command after initializing LocalServices:

let local_services = Arc::new(alien_local::LocalServices::new(&state_dir)?);

// LocalServices now includes:
// - LocalFunctionManager (existing)
// - LocalStorageManager (existing)
// - LocalContainerManager (new!)
// - Other managers...
```

The `LocalContainerManager` is initialized automatically and available to controllers via `PlatformServiceProvider`.

## Next Steps

- **[Quickstart](2-quickstart.md)** - Deploy your first container
- **[Resource API](3-resource-api.md)** - Container configuration
- **[Deployment Flow](5-deployment-flow.md)** - Cloud deployment process
- **[Local Platform](../ALIEN_LOCAL.md)** - Full local platform design

