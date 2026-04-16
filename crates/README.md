# Alien Crates

## Foundation

| Crate | Description |
|-------|-------------|
| [alien-core](./alien-core/) | Core types shared across all crates — stack, resources, deployments, bindings, events, platforms |
| [alien-error](./alien-error/) | Structured error library with machine-readable metadata (code, retryable, internal, HTTP status) |
| [alien-error-derive](./alien-error-derive/) | `#[derive(AlienErrorData)]` proc macro for alien-error |
| [alien-macros](./alien-macros/) | Proc macros — `#[controller]` for infra state machines, `#[alien_event]` for event tracking |
| [alien-permissions](./alien-permissions/) | Permission sets (JSONC), cross-cloud IAM variable interpolation, policy evaluation |

## Infrastructure & Deployment

| Crate | Description |
|-------|-------------|
| [alien-infra](./alien-infra/) | Provisioning engine — resource controllers that reconcile desired vs actual cloud state |
| [alien-deployment](./alien-deployment/) | Deployment state machine — `step()` function that drives the full lifecycle |
| [alien-preflights](./alien-preflights/) | Pre-deployment checks (compile-time, runtime, compatibility) and stack mutations |
| [alien-build](./alien-build/) | Containerizes Alien applications — builds OCI images from source code |

## Control Plane

| Crate | Description |
|-------|-------------|
| [alien-manager](./alien-manager/) | Manager server — REST API, deployment loop, release management, OCI registry proxy |
| [alien-commands](./alien-commands/) | Remote commands — invoke code on deployments without inbound networking |
| [alien-commands-client](./alien-commands-client/) | Rust client for invoking remote commands on deployments |

## Runtime & Execution

| Crate | Description |
|-------|-------------|
| [alien-runtime](./alien-runtime/) | In-container runtime — starts user code, injects bindings via gRPC, routes requests |
| [alien-bindings](./alien-bindings/) | Platform-agnostic binding traits and providers (storage, KV, vault, queue, etc.) |
| [alien-sdk](./alien-sdk/) | Public Rust SDK for Alien applications — re-exports alien-bindings |
| [alien-agent](./alien-agent/) | Pull-model agent — syncs with manager, runs deployments in remote environments |

## Cloud Clients

Custom HTTP clients that talk directly to cloud APIs using `reqwest` with per-cloud auth (AWS SigV4, GCP JWT/Bearer, Azure token). Not wrappers around official cloud SDKs — minimal dependencies, WASM-compatible, trait-based for testability.

| Crate | Description |
|-------|-------------|
| [alien-client-core](./alien-client-core/) | Shared HTTP utilities, retry logic, and response handling |
| [alien-client-config](./alien-client-config/) | Credential loading and configuration across AWS, GCP, Azure, Kubernetes |
| [alien-aws-clients](./alien-aws-clients/) | AWS — Lambda, S3, DynamoDB, SQS, ECR, IAM, CloudFormation, EC2, STS, and more |
| [alien-gcp-clients](./alien-gcp-clients/) | GCP — Cloud Run, GCS, Firestore, Pub/Sub, IAM, Artifact Registry, Compute, and more |
| [alien-azure-clients](./alien-azure-clients/) | Azure — Container Apps, Blob, Service Bus, Key Vault, VMSS, Managed Identity, and more |
| [alien-k8s-clients](./alien-k8s-clients/) | Kubernetes — Deployments, Jobs, Pods, Secrets, Services |

## CLI

| Crate | Description |
|-------|-------------|
| [alien-cli](./alien-cli/) | Developer CLI — `alien dev`, `alien build`, `alien release`, `alien deploy`, `alien serve` |
| [alien-cli-common](./alien-cli-common/) | Shared CLI utilities (networking, TUI) used by both CLIs |
| [alien-deploy-cli](./alien-deploy-cli/) | Deployment CLI for customer admins — `alien-deploy up/down/status/list/agent` |

## Local Platform

| Crate | Description |
|-------|-------------|
| [alien-local](./alien-local/) | Local platform — native process execution, filesystem storage, sled KV, in-process OCI registry |

## Testing

| Crate | Description |
|-------|-------------|
| [alien-test](./alien-test/) | E2E test harness — TestManager, TestDeployment, cross-cloud testing utilities |
| [alien-test-app](./alien-test-app/) | Minimal test application for runtime and build system tests |
