# Container Update Flow

How container and cluster configuration changes propagate during deployment.

## When Updates Fire

Each deployment compares the current resource config against the previous deployment's config. When a diff is detected, the resource's Update handler runs instead of the Create handler. This applies to both Container and ContainerCluster resources.

## Container Config Updates

When container configuration changes between deploys, the Container update handler pushes the new config to Horizon.

**Update flow:**

```
UpdateStart → UpdatingHorizonContainer → Ready
```

The handler calls Horizon's `update_container()` API with the new configuration. Horizon handles rolling restarts — Alien doesn't need to manage individual replica replacement.

**Mutable fields** (changes trigger update):

| Field | Description |
|-------|-------------|
| `code` | Container image |
| `cpu` | CPU request (min/desired) |
| `memory` | Memory request (min/desired) |
| `ephemeral_storage` | Ephemeral storage request |
| `gpu` | GPU specification |
| `replicas` | Replica count (stateful containers) |
| `autoscaling` | Scaling config (min/max replicas, target CPU/memory/HTTP metrics) |
| `environment` | Environment variables |
| `health_check` | Health probe configuration |
| `command` | Command override |

**Immutable fields** (changing these requires recreating the container):

| Field | Why |
|-------|-----|
| `stateful` | Switches between Deployment and StatefulSet semantics |
| `ports` | Requires load balancer reconfiguration |
| `pool` | Capacity group assignment is structural |

## ContainerCluster Structural Updates

When capacity groups change — added, removed, or resized — the ContainerCluster update handler performs a multi-step state flow.

```
UpdateStart
  → UpdatingOtlpSecrets
  → SyncingHorizonCapacityGroups
  → ResizingExistingAsgs
  → DeletingRemovedAsgs
  → CreatingNewLaunchTemplates
  → CreatingNewAsgs
  → WaitingForNewGroupsReady
  → CreatingNewLaunchTemplateVersion  (if template inputs changed)
  → TriggeringRollingUpdate           (if template inputs changed)
  → WaitingForRollingUpdateComplete   (if template inputs changed)
  → Ready
```

**Step details:**

1. **UpdatingOtlpSecrets** — Updates OTLP auth headers in the cloud secret store if monitoring config changed.

2. **SyncingHorizonCapacityGroups** — Calls Horizon API to sync capacity group definitions. Adds new groups, removes deleted ones at the Horizon scheduling level.

3. **ResizingExistingAsgs** — For capacity groups that still exist but changed `min_size` or `max_size`, updates the ASG/MIG/VMSS limits.

4. **DeletingRemovedAsgs** — Finds capacity groups present in the previous config but absent in the new config. Deletes their ASGs.

5. **CreatingNewLaunchTemplates** — For newly added capacity groups, creates launch templates with the selected instance type, horizond binary, and monitoring config.

6. **CreatingNewAsgs** — Creates Auto Scaling Groups for the new capacity groups.

7. **WaitingForNewGroupsReady** — Polls new ASG instances until healthy. Timeout: 15 minutes (30 polls × 30 seconds).

8-10. **Rolling update** — If `template_inputs` changed (see below), creates new launch template versions and triggers rolling machine replacement.

### Adding Capacity Groups

New capacity groups are detected when the config contains a group ID not present in the existing `asg_states`. The handler creates a launch template and ASG, then waits for instances to become healthy before proceeding.

This happens automatically when the `ContainerClusterMutation` preflight adds a new group — for example, when a GPU container is added to an existing stack.

### Removing Capacity Groups

Groups present in `asg_states` but absent in the new config are deleted. The handler first syncs with Horizon (so it stops scheduling to the removed group), then deletes the ASG.

### Resizing Capacity Groups

When `min_size` or `max_size` changes for an existing group, the handler updates the ASG limits directly. No launch template changes or rolling updates are needed.

## Rolling Machine Replacement

When the launch template inputs change, existing machines must be replaced with updated ones. This uses cloud-native rolling update mechanisms.

### What Triggers Rolling Replacement

The `TemplateInputs` struct captures deployment-time values that are baked into VM launch templates:

| Field | Purpose |
|-------|---------|
| `horizond_download_base_url` | URL for the horizond binary |
| `horizon_api_url` | Horizon API endpoint |
| `horizond_binary_hash` | ETag of the horizond binary (changes on every build) |
| `monitoring_logs_endpoint` | OTLP logs endpoint URL |
| `monitoring_metrics_endpoint` | OTLP metrics endpoint URL |
| `monitoring_auth_hash` | SHA-256 hash of OTLP logs auth header |
| `monitoring_metrics_auth_hash` | SHA-256 hash of OTLP metrics auth header |

These values are stamped onto the ContainerCluster config by `stamp_template_inputs()` during deployment — they're not user-provided. When any field differs from the previous deployment, a rolling update is triggered.

Sensitive values (auth headers) are SHA-256 hashed — only the hash is stored in the resource config, not the plaintext. The actual credentials are stored in cloud secret stores and read by horizond at boot time.

### Cloud-Native Rolling Updates

**AWS** — Instance Refresh:

```
min_healthy_percentage: 100  (always keep all instances healthy)
max_healthy_percentage: 110  (allow 10% temporary overage)
strategy: Rolling
```

Each ASG gets a new launch template version, then an Instance Refresh replaces machines one at a time. Timeout: 30 minutes (60 polls × 30 seconds).

**GCP** — MIG Rolling Update. **Azure** — VMSS Rolling Upgrade. Same principle: the cloud provider handles draining old instances and launching new ones.

### Zero-Downtime Guarantee

horizond handles graceful shutdown on SIGTERM:

1. Stop accepting new container assignments
2. Drain running containers (migrate to other machines)
3. Deregister from Horizon
4. Exit

The rolling update mechanism ensures at least 100% of desired capacity is healthy at all times. Combined with horizond's graceful shutdown, containers experience no downtime during machine replacement.

## Change Detection: How Diffs Work

Alien uses Rust's `PartialEq` derivation on config structs. The deployment executor compares the new config against the previous deployment's config field by field:

- **Container**: If any mutable field differs → Container Update handler fires
- **ContainerCluster**: If capacity groups differ → structural update. If `template_inputs` differs → rolling replacement. Both can happen in the same deployment.
- **No change**: Resource is skipped entirely (no handler runs)

## Capacity Group Adaptation

The `ContainerClusterMutation` preflight runs on every deploy (not just the first). It analyzes containers and adds missing capacity groups:

- Container needs a GPU → `gpu` group created if it doesn't exist
- Container needs >200GB ephemeral storage → `storage` group created
- Default containers → `general` group

This means adding a GPU container to an existing stack automatically creates a GPU capacity group in the cluster, which the update handler then provisions as a new ASG.

See [Preflights](../01-provisioning/02-preflights.md) for the full mutation execution order.
