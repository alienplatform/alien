# Architecture

Alien is a framework for deploying and running managed software in remote environments. This document explains how the codebase fits together and where to start reading.

An Alien application starts with an `alien.ts` file. It looks like this:

```typescript
// alien.ts
import * as alien from "@alienplatform/core"

const storage = new alien.Storage("uploads").build()

const fn = new alien.Function("processor")
  .code({ type: "source", toolchain: { type: "typescript" }, src: "." })
  .link(storage)
  .build()

export default new alien.Stack("my-app")
  .add(storage, "frozen")
  .add(fn, "live")
  .build()
```

This file — combined with your application code — is everything Alien needs to build, deploy, and keep running your software. The sections below explain what happens after you write it.

## How the Pieces Fit Together

**Defining resources.** The `alien.ts` above produces a `Stack` — a description of cloud-agnostic resources (Functions, Storage, KV, Queues, etc.) defined using `@alienplatform/core`.

**Building.** `alien-build` reads the Stack, compiles source code with the right toolchain (TypeScript via Bun, Rust via cargo-zigbuild), and packages each compute resource as an OCI image containing `alien-runtime` + the app binary.

**Deploying.** Once built, the Stack gets deployed to a remote environment and kept running there. Two crates handle this together:

`alien-infra` is the low-level engine. It provisions each resource one step at a time — each step calls one cloud API, saves state, and returns:

```
CreateBucket → ConfigureVersioning → ConfigureLifecycle → Ready
     ↓                 ↓                      ↓              ↓
  AWS API           AWS API               AWS API         done
  (save state)      (save state)          (save state)
```

Because state is serialized after every step, execution can pause and resume on a completely different machine. Initial setup might run on a customer's laptop; ongoing updates run from a remote orchestrator. Crashes are safe — the next run picks up exactly where the last one stopped. → [01-provisioning/00-infra.md](../01-provisioning/00-infra.md)

`alien-deployment` sits on top and handles the higher-level lifecycle — preflights, frozen resources first, then live resources, secrets sync, health checks, heartbeating forever. → [01-provisioning/01-deployment.md](../01-provisioning/01-deployment.md)

**Running.** `alien-runtime` is the optional entry point for your containers and functions. Without it, a container just runs your code. With it, your app gets:

```typescript
// Instead of AWS-specific code:
const s3 = new S3Client({ region: "us-east-1" })
await s3.putObject({ Bucket: "my-bucket", Key: "file.json", Body: data })

// You write platform-agnostic code that works on AWS, GCP, or local:
await storage("uploads").put("file.json", data)
```

Plus: react to events (file uploads, queue messages, cron), receive remote commands, graceful shutdown. → [04-runtime/](../04-runtime/), [02-capabilities.md](02-capabilities.md)

**Commands.** `alien-commands` lets the control plane send commands to deployments. Uses platform-native push (Lambda invoke, Pub/Sub, Service Bus) or outbound polling — same envelope format either way. → [04-runtime/02-commands.md](../04-runtime/02-commands.md)

**The control plane.** `alien-manager` ties everything together. It stores releases and deployment state, runs the deployment loop (calling `alien-deployment::step()` repeatedly), hosts the command server for remote commands, collects telemetry, and manages authentication. It's the process that actually drives deployments in remote environments. → [02-manager/](../02-manager/)

**Permissions.** `alien-permissions` defines permission sets as JSONC files. `alien-preflights` turns stack permission profiles into ServiceAccount resources. `alien-infra` service account controllers compile them to IAM Roles / GCP Service Accounts / Azure Managed Identities. → [04-permissions.md](04-permissions.md)

## End-to-End Flow

Here's what happens when you deploy an Alien application to a customer's AWS account:

```
Developer                    alien-manager                   Customer's AWS
     │                            │                              │
  alien build                     │                              │
  alien release ────────────────▶ │  Store release + images      │
                                  │                              │
  alien deploy ─────────────────▶ │  Create deployment           │
                                  │                              │
                                  │── deployment loop ──────────▶│
                                  │   step(): create IAM role    │
                                  │   step(): create Lambda      │
                                  │   step(): create S3 bucket   │
                                  │   ...                        │
                                  │                              │
                                  │   status: running ◀──────────│
                                  │                              │
  alien command invoke ──────────▶│── command ─────────────────▶│
                                  │◀── response ────────────────│
```

1. **Build** — `alien build` compiles your `alien.ts` into OCI images locally
2. **Release** — `alien release` pushes images to a platform-specific registry (ECR for AWS) using your local cloud credentials, and creates a release record on alien-manager
3. **Deploy** — `alien deploy` creates a deployment record. alien-manager's deployment loop picks it up, impersonates a service account in the customer's AWS account, and calls `alien-deployment::step()` repeatedly until everything is provisioned
4. **Running** — The deployment is running in the customer's cloud. alien-manager monitors it, dispatches commands, and ships updates when you push new releases

For environments where alien-manager can't call cloud APIs directly (Kubernetes, airgapped), an Agent in the remote environment polls alien-manager for updates and deploys locally. Same `step()` function, different caller.

## Reading Order

If you're new, read the directories in order:

### [00-foundations/](./) — What Alien is and what it does

- **[01-stack-and-resources.md](01-stack-and-resources.md)** — Stacks, Resources, frozen vs live
- **[02-capabilities.md](02-capabilities.md)** — What apps can do: bindings, events, commands
- **[03-build.md](03-build.md)** — How source code becomes OCI images
- **[04-permissions.md](04-permissions.md)** — Permission sets, profiles, IAM policy generation

### [01-provisioning/](../01-provisioning/) — How resources get created and kept alive

- **[00-infra.md](../01-provisioning/00-infra.md)** — The step-based provisioning engine (`alien-infra`)
- **[01-deployment.md](../01-provisioning/01-deployment.md)** — Full deployment lifecycle (`alien-deployment`)
- **[02-preflights.md](../01-provisioning/02-preflights.md)** — Validation checks and stack mutations
- **[03-environment-variables.md](../01-provisioning/03-environment-variables.md)** — Plain vs secret vars, vault syncing
- **[04-networking.md](../01-provisioning/04-networking.md)** — VPC/VNet controllers

### [02-manager/](../02-manager/) — The control plane

- **[00-overview.md](../02-manager/00-overview.md)** — Architecture, provider traits, builder API
- **[01-deployments.md](../02-manager/01-deployments.md)** — Deployment lifecycle and state machine
- **[02-releases.md](../02-manager/02-releases.md)** — Release model and artifact registry
- **[03-commands.md](../02-manager/03-commands.md)** — Remote command protocol
- **[04-telemetry.md](../02-manager/04-telemetry.md)** — OTLP ingestion and forwarding
- **[05-auth.md](../02-manager/05-auth.md)** — Token security and scope enforcement
- **[06-api.md](../02-manager/06-api.md)** — Complete API endpoint reference
- **[07-running.md](../02-manager/07-running.md)** — Configuration and environment variables
- **[08-local-development.md](../02-manager/08-local-development.md)** — How `alien dev` works end-to-end

### [03-cli/](../03-cli/) — Command-line interface

- **[00-overview.md](../03-cli/00-overview.md)** — CLI architecture: modes, commands, config

### [04-runtime/](../04-runtime/) — What runs inside deployed containers

- **[00-runtime.md](../04-runtime/00-runtime.md)** — The `alien-runtime` entry point and transport layer
- **[01-bindings.md](../04-runtime/01-bindings.md)** — How bindings work internally (gRPC, providers)
- **[02-commands.md](../04-runtime/02-commands.md)** — Remote Commands: envelope format, push vs poll
- **[03-wait-until.md](../04-runtime/03-wait-until.md)** — Background task coordination

### [05-platforms/](../05-platforms/) — Platform-specific quirks

- **[00-aws.md](../05-platforms/00-aws.md)** — AssumeRole, Lambda, S3 naming, networking
- **[01-gcp.md](../05-platforms/01-gcp.md)** — Service account impersonation, API enablement, resource-level IAM
- **[02-azure.md](../05-platforms/02-azure.md)** — UAMI + FIC cross-tenant access, Resource Groups, no default VNet
- **[03-local.md](../05-platforms/03-local.md)** — Local platform without cloud APIs
- **[04-kubernetes.md](../05-platforms/04-kubernetes.md)** — Pull-only, external bindings

### Reference

- **[06-resources/](../06-resources/)** — Per-resource reference: compute, KV, queue, vault, network, containers
- **[07-containers/](../07-containers/)** — Deep dive on container orchestration with Horizon
- **[08-sdk/](../08-sdk/)** — Per-language SDK reference (TypeScript, Rust, Python)
- **[09-testing/](../09-testing/)** — Testing framework and E2E test strategy
- **[10-guides/](../10-guides/)** — Adding new resources, coding guidelines
