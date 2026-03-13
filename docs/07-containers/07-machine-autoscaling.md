# Containers - Machine Autoscaling

How Alien automatically scales infrastructure (VMs) based on Horizon's capacity planning.

## Two-Level Autoscaling

Alien Containers use a two-level autoscaling system:

1. **Replica Scaling** (fast, every 5 seconds) - Horizon adds/removes containers
2. **Machine Scaling** (slow, every 60 seconds) - Alien adds/removes VMs per group

This separation enables:
- Quick response to traffic spikes (add containers)
- Efficient resource usage (add machines only when needed)
- Cost optimization (remove idle machines)
- Heterogeneous fleets (different instance types in one cluster)

| Level | Component | Interval | What It Scales |
|-------|-----------|----------|----------------|
| **Replica** | Horizon scheduler | 5 seconds | Containers (within existing capacity per group) |
| **Machine** | Alien infrastructure actor | 60 seconds | VMs per capacity group (when capacity needed) |

## Capacity Groups

A single Horizon cluster can have multiple **capacity groups** - sets of machines with different instance types:

**Example:**
```
Horizon Cluster: production-app
  ├─ Capacity Group "general" → myapp-general-asg (m7g.2xlarge)
  │  └─ Containers: api, worker
  │
  └─ Capacity Group "storage" → myapp-storage-asg (i4i.2xlarge)
     └─ Containers: quickwit, clickhouse
```

All machines join the same WireGuard mesh and DNS namespace, but Alien scales each ASG independently based on its group's capacity needs.

## Machine Autoscaling Algorithm

Alien's infrastructure controller polls Horizon's **capacity plan API** and applies scaling decisions to ASGs.

### How It Works

Every time Horizon's scheduler runs (every 5 seconds), it calculates capacity recommendations for each capacity group based on:

**Priority 1: Unschedulable replicas** (urgent)
- If replicas can't be placed → calculate machines needed → scale up immediately

**Priority 2: Utilization** (proactive optimization)  
- If >85% utilized → scale up by 1 machine
- If <30% utilized (sustained for 5 minutes) → scale down by 1 machine
- Between 30-85% → no change (stable band prevents oscillation)

**Alien's role:**
- Poll capacity plan every 60 seconds
- Apply recommendations to each ASG
- No decision logic - just execute what Horizon calculated

### ContainerClusterController Scaling Logic

```rust
// In alien-deployment::step() for Running status
// ContainerClusterController::ready() handler

async fn ready(&mut self, ctx: &ResourceControllerContext) -> HandlerAction {
    let config = ctx.desired_resource_config::<ContainerCluster>();
    
    // Get Horizon cluster configuration
    let horizon_config = ctx.horizon_config.unwrap();
    let cluster_config = horizon_config.clusters.get(&config.id).unwrap();
    
    let horizon_client = HorizonClient::new(&horizon_config.url);
    
    // Ask Horizon: "What infrastructure do I need?"
    let capacity_plan = horizon_client.get_capacity_plan(
        &cluster_config.cluster_id,
        &cluster_config.management_token,
    ).await?;
    
    // Apply Horizon's decisions to each ASG
    for group_plan in capacity_plan.groups {
        let asg_name = self.asg_names.get(&group_plan.group_id)
            .ok_or_else(|| anyhow!("No ASG found for group {}", group_plan.group_id))?;
        
        if group_plan.desired_machines != group_plan.current_machines {
            let asg_client = ctx.service_provider.get_aws_asg_client();
            
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
    
    // Poll Horizon for container status
    let containers = horizon_client.list_containers(
        &cluster_config.cluster_id,
        &cluster_config.management_token,
    ).await?;
    
    // Check if containers are running
    let all_running = containers.iter().all(|c| c.status == "running");
    
    if all_running {
        // Deployment complete
        ctx.agent_state.status = Some(AgentStatus::Running);
    }
    
    Ok(HandlerAction::Stay {
        suggested_delay: Some(Duration::from_secs(60)),
    })
}
```

**How it works:** Alien polls Horizon's capacity plan and applies recommendations to ASGs. No decision logic - just executes what Horizon calculated.

## Scale-Up Example

**Starting state:**
- Capacity group "general": 2 machines (i-abc123, i-def456)
- API container: 8 replicas running, wants 10 replicas (autoscaling)
- Horizon scheduler can't place 2 more replicas (no capacity)

**T+0s: Horizon can't place replicas**
```rust
// Horizon scheduler tries to scale API from 8 to 10 replicas
let machine = find_machine_with_capacity(&api_container, general_machines);
// Returns: None (all general machines full)

// Scheduler records unschedulable work
schedulingFailures["api"] = { attemptedReplicas: 2 };
```

**T+60s: Alien polls capacity plan**
```rust
// Acquires lock, calls alien-deployment::step()
// ContainerClusterController queries Horizon capacity plan API
let plan = horizon_client.get_capacity_plan(...).await?;

// Horizon responds:
// {
//   "groups": [{
//     "groupId": "general",
//     "currentMachines": 2,
//     "desiredMachines": 3,
//     "reason": "need_capacity",
//     "unschedulableReplicas": 2,
//     "utilizationPercent": 85.0
//   }]
// }

// Alien applies Horizon's recommendation
asg_client.set_desired_capacity("myapp-general-asg", 3);
```

**T+90s: ASG launches new instance**
```
EC2 instance i-ghi789 launching in us-east-1c
```

**T+210s: New machine boots and joins cluster**
```rust
// horizond on i-ghi789 sends heartbeat to Horizon
POST /heartbeat
{
  "clusterId": "cluster-abc123",
  "machineId": "i-ghi789",
  "capacityGroup": "general",
  "totalCpu": 8.0,
  "availableCpu": 8.0,
  ...
}
```

**T+215s: Horizon scheduler assigns remaining replicas**
```rust
// Horizon sees new machine with capacity in "general" group
// Assigns 2 remaining replicas to i-ghi789
```

**T+245s: 10 replicas running**
```
3 machines in "general" group:
  i-abc123: 3 replicas
  i-def456: 3 replicas  
  i-ghi789: 4 replicas
Total: 10 replicas serving traffic
```

**Total infrastructure scale-up time: ~4 minutes** (from decision to new replica serving)

**Key: No deadlock possible.** Horizon always exposes unschedulable work via capacity plan. Alien always applies what Horizon recommends.

## Scale-Down Example

**Starting state:**
- Capacity group "general": 3 machines (i-abc123, i-def456, i-ghi789)
- API container: Scaled down from 10 to 4 replicas (low CPU usage)

**T+0s: Horizon removes replicas**
```rust
// Horizon scheduler detects low CPU (40%)
// Scales down to 4 replicas
// Machine i-ghi789 now has minimal load (1 replica)
```

**T+6m0s: Alien polls capacity plan (after sustained low utilization)**
```rust
// Query Horizon capacity plan
let plan = horizon_client.get_capacity_plan(...).await?;

// Horizon responds (after 5 minutes of sustained low utilization):
// {
//   "groups": [{
//     "groupId": "general",
//     "currentMachines": 3,
//     "desiredMachines": 2,
//     "reason": "low_utilization",
//     "utilizationPercent": 35.0
//   }]
// }

// Alien applies: scale down from 3 to 2
asg_client.set_desired_capacity("myapp-general-asg", 2);
```

**T+6m30s: ASG picks instance for termination**
```
ASG selects i-ghi789 for termination (least replicas, newest machine)
ASG sends SIGTERM to horizond
```

**T+6m30s: horizond enters draining mode**
```rust
// horizond receives SIGTERM
// Sends heartbeat with status="draining"
// Horizon scheduler stops assigning new work to this machine
// Scheduler migrates replica from i-ghi789 to i-abc123 or i-def456
```

**T+7m0s: Replica migrated, instance terminates**
```
2 machines remaining in "general" group:
  i-abc123: 2 replicas
  i-def456: 2 replicas
Total: 4 replicas
```

**Total infrastructure scale-down time: ~8 minutes** (5min sustained detection + 60s poll + 2-3min graceful drain)

**Key: Conservative scale-down.** Horizon requires sustained low utilization (5 minutes) before recommending scale-down, preventing flapping. Draining status ensures replicas are migrated before termination.

## Integration Points

### Alien Reads from Horizon

**What Alien needs to know:**
- Capacity plan (desired machines per group)
- Container status (for deployment completion)

**API calls:**
```rust
// Get capacity plan (for machine autoscaling)
GET /clusters/{cluster_id}/capacity-plan
Authorization: Bearer {management_token}

// Response:
// {
//   "groups": [
//     { 
//       "groupId": "general", 
//       "currentMachines": 3, 
//       "desiredMachines": 5, 
//       "reason": "need_capacity",
//       "unschedulableReplicas": 8,
//       "utilizationPercent": 88.5
//     },
//     { 
//       "groupId": "storage", 
//       "currentMachines": 1, 
//       "desiredMachines": 1, 
//       "reason": "at_target",
//       "utilizationPercent": 45.0
//     }
//   ]
// }

// Get container status (for deployment health)
GET /clusters/{cluster_id}/containers/{name}
Authorization: Bearer {management_token}
```

### Alien Writes to Horizon

**What Alien provides to Horizon:**
- Cluster definition with capacity groups (at setup)
- Container definitions (image, resources, scaling config, group assignment)
- Volume registrations (for stateful containers)
- Load balancer targets (for exposed containers)
- Environment variables (from `.link()`, secrets, etc.)

**API calls:**
```rust
// Create cluster with capacity groups (setup phase)
POST /clusters
Authorization: Bearer {platform_jwt}

// Create container with group assignment
POST /clusters/{cluster_id}/containers
Authorization: Bearer {management_token}

// Update container (rolling updates)
PATCH /clusters/{cluster_id}/containers/{name}
Authorization: Bearer {management_token}
```

## Cost Implications

### Replica Scaling (Instant, Free)

Adding/removing replicas is **free** - you're just using existing machine capacity.

**Example:**
```
2 machines (m7g.2xlarge): $0.652/hr
  2 replicas: $0.652/hr
  8 replicas: $0.652/hr (same cost!)
```

This is Horizon's responsibility.

### Machine Scaling (Costly, Horizon Plans)

Adding/removing machines changes cost. Horizon calculates what's needed, Alien applies it.

**Example (single group):**
```
Baseline (2 machines): $476/month
  ├─ 2 × m7g.2xlarge × $0.326/hr × 730 hrs

Peak (4 machines): $952/month
  └─ 4 × m7g.2xlarge × $0.326/hr × 730 hrs

Savings from autoscaling:
If peak traffic is 20% of time:
  Cost = (0.8 × $476) + (0.2 × $952) = $571/month
  Savings = $952 - $571 = $381/month (40% reduction)
```

**Example (multiple groups):**
```
Baseline:
  general: 2 × m7g.2xlarge = $476/month
  storage: 0 × i4i.2xlarge = $0/month (scale-to-zero)
  Total: $476/month

Peak:
  general: 4 × m7g.2xlarge = $952/month
  storage: 2 × i4i.2xlarge = $1,124/month
  Total: $2,076/month

Savings from per-group autoscaling:
  Storage scales to zero when idle (not searching)
  General scales with traffic
  Each group independently optimized
```

This is Alien's responsibility (adjusting ASG desired capacity per group based on Horizon's capacity plan).

## Best Practices

**1. Set appropriate group min/max sizes:**
```json
// General workloads: Higher min for HA
{
  "groupId": "general",
  "minSize": 2,  // Always 2 machines (HA across zones)
  "maxSize": 10  // Can scale up to handle traffic
}

// Expensive workloads: Lower min to save cost
{
  "groupId": "gpu",
  "minSize": 0,  // Scale to zero when idle
  "maxSize": 3   // Limit expensive GPU instances
}
```

**2. Monitor capacity plan decisions:**
- Check Horizon's capacity plan reasons (`need_capacity`, `low_utilization`)
- Verify ASG scaling matches Horizon's recommendations
- Alert on prolonged `need_capacity` state (may indicate quota limits)

**3. Use multiple groups for heterogeneous workloads:**
```
Single group: All containers share m7g.2xlarge instances
  → Wasteful if you have diverse workloads

Multiple groups: Right instance type for each workload
  → general: m7g.2xlarge for APIs
  → storage: i4i.2xlarge for search engines (NVMe)
  → gpu: p4d.24xlarge for ML (A100 GPUs)
```

## Scaling Responsiveness

**Replica scaling (Horizon - fast):**
```
T+0s:    Traffic spike, CPU → 85%
T+5s:    Metrics reported to Horizon
T+10s:   Scheduler scales replicas (within existing capacity)
T+40s:   New replicas serving traffic
```
**Total: ~40 seconds** - Quick response using existing machines

**Machine scaling (Alien - measured):**
```
T+0s:    Scheduler can't place replicas → unschedulable
T+0s:    Capacity plan updated: "need 1 more machine"
T+60s:   Alien polls, sees recommendation
T+60s:   Alien scales ASG
T+4m0s:  New machine boots, replicas placed
```
**Total: ~4 minutes** - As fast as cloud infrastructure allows

The 60-second poll adds minimal latency (~25% overhead) compared to the 3+ minute machine boot time. This is an acceptable trade-off for simpler architecture.

## Scaling Stability

Horizon prevents infrastructure flapping through:

**Wide utilization thresholds:**
- Scale up: >85% (not >80%)
- Scale down: <30% (not >40%)
- Stable band: 30-85% (55 percentage points!)

**Sustained signals for scale-down:**
- Requires 5 minutes of consistent low utilization
- Prevents reacting to brief traffic dips

**Cooldown periods:**
- After scale-up: 5 minutes before next scale-up (prevents re-evaluation during machine boot)
- After scale-down: 5 minutes before next scale-down
- After scale-down: 5 minutes before scale-up (prevents immediate bounce)

**Why 5 minutes for scale-up cooldown?**

Machine boot time is typically 3-4 minutes. The 5 minute cooldown ensures:

1. **Prevents estimation drift over-scaling:**
   - Scheduler keeps running every 5 seconds while machines boot
   - Capacity estimation might change slightly during this time
   - Without cooldown: Changed estimate could trigger additional scale-up
   - With 5min cooldown: Machines have time to boot and be counted before re-evaluation

2. **Trade-off accepted:**
   - Downside: If workload genuinely increases during boot, adds 5min delay to next scale-up
   - Upside: Prevents over-provisioning from estimation changes (saves cost)
   - Philosophy: Conservative scaling is better than over-scaling at small scale

**Result:** Infrastructure changes are deliberate and stable, not twitchy. Machines don't churn every few seconds.

## Crash Loop Protection & Auto-Recovery

Horizon provides automatic crash loop detection and recovery, matching Kubernetes behavior.

### How Crash Loops Work

When containers crash repeatedly:

```
Crash #1 → 30s backoff → Retry
Crash #2 → 1min backoff → Retry
Crash #3 → 2min backoff → Mark as "crashloop" → Retry
Crash #4 → 4min backoff → Retry
Crash #5+ → 5min backoff (capped) → Retry indefinitely
```

**Key behaviors:**

1. **Never permanently failed** - Unlike older systems, containers in `crashloop` status keep retrying forever with exponential backoff (capped at 5 minutes)

2. **Auto-recovers when capacity improves** - If crashes are infrastructure-related (OOM from overloaded machines), containers automatically recover when new machines come online

3. **Resets after success** - If replicas run successfully for 5+ minutes, crash counter resets to 0

### Auto-Recovery Scenario

**Common deadlock scenario (now solved):**

```
T+0s:   2 machines at 95% CPU
T+10s:  New container deployed, needs 2 CPUs
T+10s:  Scheduler: "Can't place replica" → unschedulableReplicas = 1

T+60s:  Alien scales ASG from 2 → 3 machines
T+70s:  Scheduler finds "capacity" on overloaded machine → schedules replica
T+71s:  Replica OOMs (machine overloaded) → Crash #1, backoff 30s

T+101s: Retry → OOM → Crash #2, backoff 1min
T+161s: Retry → OOM → Crash #3, status = "crashloop", backoff 2min

T+4m0s:  🎉 New machine finally boots with plenty of capacity
T+5m41s: Backoff expires → Scheduler places replica on new machine
T+5m31s: ✅ Replica succeeds! Auto-recovered from crashloop
```

**Why this works:**
- ✅ Crashloop is non-terminal (keeps retrying)
- ✅ Backoff allows infrastructure time to stabilize
- ✅ Scheduler uses new capacity when it becomes available
- ✅ No manual intervention required

### Status Transitions

```
pending → running → pending (scale down)
   ↓                    ↓
crashloop (3+ crashes, keeps retrying)
   ↓                    ↓
running (auto-recovery when capacity/infrastructure improves)
```

**stopped** status is different - it's set manually via API and tells the scheduler to permanently stop scheduling replicas (no automatic recovery).

### When to Manually Stop a Container

If a container is truly broken (bad image, code bugs), use the API to stop it:

```
PATCH /clusters/{cluster_id}/containers/{name}
{ "status": "stopped" }
```

This tells the scheduler: "Don't retry this container, even if capacity becomes available."

## Limitations

**Current limitations:**
1. Reactive scaling only (no ML prediction of future demand)
2. No scheduled scaling (e.g., scale up before known peak hours)
3. Crash loop protection doesn't distinguish between infrastructure failures (OOM) and application failures (code bugs) - treats all crashes the same

**Future enhancements:**
- Prediction-based capacity planning (ML-driven forecasting)
- Scheduled capacity changes (cron-based minSize adjustments)
- Spot instance integration (mix on-demand + spot per group)
- Smart crash detection (distinguish OOM from application errors)

## Architecture Summary

**Clean separation of concerns:**

| Component | Responsibility | Example |
|-----------|---------------|---------|
| **Horizon Scheduler** | Calculate desired replicas and machines per group | "Need 10 api replicas, can only place 8 → need 3 general machines" |
| **Alien Infrastructure Actor** | Apply capacity plan to ASGs | `asg.set_desired_capacity("general-asg", 3)` |

**Why this works:**
- ✅ Horizon has complete workload context (metrics, constraints, scheduling failures)
- ✅ Alien has cloud-specific knowledge (instance types, quotas, ASGs)
- ✅ Clear API boundary (capacity plan endpoint)
- ✅ No deadlocks possible (unschedulable work immediately visible in scheduler output)
- ✅ Simple (scheduler calculates everything in one pass)

For structural capacity group changes (add/remove/resize) and rolling machine replacement during updates, see [Update Flow](10-update-flow.md).

## Next Steps

- **[Update Flow](10-update-flow.md)** - How cluster updates propagate
- **[Deployment Flow](5-deployment-flow.md)** - See Alien and Horizon working together
- **[Infrastructure](4-infrastructure.md)** - How Alien selects instance types per group
- **[Resource API](3-resource-api.md)** - Container configuration options

