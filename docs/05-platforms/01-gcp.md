# GCP Platform

Platform-specific details for working on GCP controllers.

## Cross-Account Access

GCP uses **service account impersonation**. The managing service account is granted `roles/iam.serviceAccountTokenCreator` on the target project's service account, allowing it to generate short-lived tokens.

## Resource Mapping

| Alien Resource | GCP Service |
|---|---|
| Function | Cloud Run |
| Container | GCE instances (via Horizon) |
| Storage | Cloud Storage |
| KV | Firestore |
| Queue | Pub/Sub |
| Vault | Secret Manager |
| Build | Cloud Build |
| ServiceAccount | GCP Service Account |

## API Enablement

GCP requires APIs to be explicitly enabled before use. The `GcpServiceActivationMutation` preflight adds `ServiceActivation` resources that enable required APIs (Cloud Run, Cloud Storage, Pub/Sub, etc.) before any resources are provisioned.

## Networking

- **Default network** exists with one regional subnet, but no NAT
- **Create mode**: Alien provisions custom-mode VPC + regional subnet + Cloud Router + Cloud NAT
- No external IPs when `accessConfigs` is empty in the Compute API — the controller explicitly adds `AccessConfig { type: ONE_TO_ONE_NAT }` only for `use-default` mode
- Cloud NAT handles egress for `create` mode (no per-instance public IPs)

## Build Targets

Default: `linux-x64`

## Permissions

Permissions go into **custom roles** bound via resource-level IAM. Each stack uses **one GCP project**, so all resources in the project belong to the stack — no CEL conditions needed. Both stack-level and resource-level scope use `setIamPolicy` directly on individual resources. For `provision` permissions (needed during initial setup when resources don't exist yet), project-level IAM bindings are used instead.

## Quirks

- IAM changes have propagation delay (up to 60 seconds). Controllers that create a role and immediately try to use it may hit permission errors.
- Firestore requires a default database to exist. The KV controller checks for this.
- Pub/Sub subscriptions are named `{function-name}-rq` for command request queues.
- Cloud Run services need the invoker IAM binding set for Pub/Sub push delivery.
- Cloud Storage bucket names are globally unique (same as S3).
