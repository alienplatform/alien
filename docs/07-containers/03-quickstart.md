# Containers - Quickstart

Deploy a simple Node.js API container in 5 minutes.

## Prerequisites

- Alien CLI installed
- Cloud credentials configured (AWS, GCP, or Azure)
- Node.js project with a simple HTTP server

## Step 1: Create Your Container Code

```bash
mkdir my-api
cd my-api
npm init -y
npm install express
```

Create `index.js`:

```javascript
const express = require('express');
const app = express();

app.get('/health', (req, res) => {
  res.json({ status: 'healthy' });
});

app.get('/api/hello', (req, res) => {
  res.json({ message: 'Hello from Alien!' });
});

const PORT = process.env.PORT || 3000;
app.listen(PORT, () => {
  console.log(`Server listening on port ${PORT}`);
});
```

## Step 2: Define the Container

Create `alien.ts`:

```typescript
import * as alien from "@alienplatform/core"

const api = new alien.Container("api")
  .code({
    type: "source",
    toolchain: { type: "node" },
    src: "."
  })
  .cpu(1)                           // 1 vCPU per replica
  .memory("2Gi")                    // 2 GB per replica
  .minReplicas(2)                   // Always run at least 2
  .maxReplicas(20)                  // Scale up to 20 under load
  .port(3000)                       // Container listens on 3000
  .expose("http")                   // Expose via HTTPS load balancer
  .build()

export default new alien.Stack("my-api")
  .add(api, "live")
  .build()
```

**What this does:**
- Builds a container image from your Node.js code
- Runs it on compute instances (EC2/GCE/Azure VMs)
- Creates a load balancer (ALB/GCP LB/Azure LB)
- Auto-scales from 2 to 20 Replicas based on CPU

## Step 3: Build

```bash
alien build
```

This:
1. Compiles your stack configuration
2. Builds a container image for your container
3. Outputs build artifacts to `.alien/` directory

**No cloud resources created yet** - this is a local build step.

## Step 4: Deploy

```bash
alien deploy production --size medium --platform aws
```

This kicks off the deployment:

### What Happens Behind the Scenes

**Phase 1: Preflights (2-3 seconds)**
- System analyzes your container requirements
- Determines you need 20 vCPU at max scale (1 vCPU × 20 replicas)
- Selects instance type: `m7g.2xlarge` (8 vCPU, 32 GB, ARM)
- Calculates pool size: min=2, max=3 machines (with 25% headroom for "medium")
- Auto-generates a ComputePool resource

**Phase 2: Initial Setup (2-5 minutes)**
- Creates IAM role for EC2 instances
- Creates launch template with machine agent
- Creates Auto Scaling Group (min=2, max=3, desired=2)
- Creates Application Load Balancer
- Pushes container image to platform's artifact registry

**Phase 3: Machines Boot (1-2 minutes)**
- ASG launches 2 EC2 instances
- User data script installs containerd and `alien` CLI
- Machines run `alien machine run` (starts agent)
- Agents register with platform: "I have 8 vCPU, 32 GB, in us-east-1a"

**Phase 4: Scheduler Assigns Replicas (5 seconds)**
- Scheduler sees: container wants min=2 Replicas
- Assigns Replica 1 → machine i-abc123
- Assigns Replica 2 → machine i-def456

**Phase 5: Containers Start (30 seconds)**
- Machines poll platform: "What should I run?"
- Platform responds: "Run api Replica 1"
- Machines pull image from artifact registry
- Start containers via containerd
- Register with load balancer

**Total time: ~5-8 minutes** for first deployment.

## Step 5: Test It

```bash
# Get the service URL from deployment output
curl http://api-lb-abc123.us-east-1.elb.amazonaws.com/api/hello

# Response:
# {"message":"Hello from Alien!"}
```

## What You Get

After deployment:

```
Infrastructure Created:
├─ Auto Scaling Group (2-3 machines)
│  ├─ Machine i-abc123 (us-east-1a)
│  │  └─ Container: api Replica 1
│  └─ Machine i-def456 (us-east-1b)
│     └─ Container: api Replica 2
│
└─ Application Load Balancer
   └─ URL: http://api-lb-abc123.us-east-1.elb.amazonaws.com
```

**Cost (AWS us-east-1):**
- 2 × m7g.2xlarge instances: ~$476/month
- Load balancer: ~$22/month
- **Total: ~$500/month** for 2 replicas with HA

## Autoscaling in Action

Send load to your API:

```bash
# Generate traffic
hey -z 5m -c 100 http://api-lb-abc123.us-east-1.elb.amazonaws.com/api/hello
```

**What happens:**
1. CPU usage rises to 85%
2. Scheduler calculates: need 3 Replicas (within 15 seconds)
3. Machines start 3rd container
4. Load balancer adds 3rd target
5. CPU drops to 60%

If traffic continues to increase:
6. Scheduler wants 9 Replicas
7. Current machines can fit 8 total (4 per machine)
8. Infrastructure Actor adds 3rd machine (within 2 minutes)
9. 9th Replica starts on new machine

When traffic drops:
- Scheduler removes excess Replicas
- Infrastructure Actor scales machines back down to min=2

## Next Steps

**Add persistent storage:**
```typescript
const postgres = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .cpu(4).memory("16Gi")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  .build()
```
See [Stateful Containers](9-stateful-services.md) for details.

**Connect to other resources:**
```typescript
const storage = new alien.Storage("data").build()
const queue = new alien.Queue("jobs").build()

const api = new alien.Container("api")
  .link(storage)   // Get ALIEN_STORAGE_DATA_* env vars
  .link(queue)     // Get ALIEN_QUEUE_JOBS_* env vars
  .build()
```

**Deploy to GCP or Azure:**
```bash
alien deploy production --platform gcp
alien deploy production --platform azure
```

## Learn More

- **[Resource API](3-resource-api.md)** - All Container configuration options
- **[Storage](5-storage.md)** - Ephemeral and persistent storage
- **[Networking](6-networking.md)** - Load balancers, cluster networking
- **[Autoscaling](8-autoscaling.md)** - Deep dive on scaling behavior

