# Releases and Artifact Registries

A release is an immutable snapshot of your application — compiled resource definitions and container image references for one or more platforms. Artifact registries store those container images. Which registry you use determines what you can deploy.

## The Release Record

```sql
CREATE TABLE releases (
  id          TEXT PRIMARY KEY,   -- "rel_" prefix, nanoid
  stack       TEXT NOT NULL,      -- JSON: { aws?: Stack, gcp?: Stack, local?: Stack, ... }
  created_at  TEXT DEFAULT CURRENT_TIMESTAMP
)
```

The `stack` field contains platform-keyed stack definitions — compiled `alien.config.ts` output as JSON. A release can contain stacks for multiple platforms simultaneously.

## Creating a Release

```
alien build --platform aws       →  OCI tarballs in .alien/build/aws/
alien release --server http://…  →  Push images to registry, create release record
```

Two steps:

1. `alien build` compiles source code into OCI image tarballs locally (see [Build](../00-foundations/03-build.md))
2. `alien release` pushes images to the configured artifact registry, then calls `POST /v1/releases` with image URIs baked into the stack JSON

alien-manager stores the release record and sets `desired_release_id` on eligible deployments (see [Deployments — Desired vs Current Release](01-deployments.md#desired-vs-current-release)).

The developer pushes images directly from their machine — alien-manager never mediates registry access.

## Artifact Registry Tiers

Not all registries are equal. Where your images live determines which resource types you can deploy and to which platforms.

### Local registry

In dev mode (`alien dev`), alien-manager starts an in-process OCI registry on disk. No cloud credentials, no network — images load directly from the local build output.

You can only deploy locally. This is what `alien dev` uses.

### Generic OCI registry

Any OCI-compatible registry — Docker Hub, GitHub Container Registry, a self-hosted registry. You push with standard Docker credentials.

This works for **Container** resources (Horizon). Horizon pulls images over the network using standard OCI protocols, so any reachable registry works.

It does **not** work for **Function** resources. Lambda, Cloud Run, and Azure Container Apps can only pull from their platform's native registry (ECR, GAR, ACR respectively). This is a hard constraint imposed by the cloud provider.

### Cloud-specific registry

ECR (AWS), Artifact Registry (GCP), or ACR (Azure). You push using cloud credentials for the target platform.

This works for **everything** — both Function and Container resources on that platform. It's the only way to deploy Function resources.

### Summary

| Registry | Container (Horizon) | Function (Lambda, Cloud Run, etc.) |
|----------|--------------------|------------------------------------|
| Local (dev mode) | Local only | Local only |
| Generic OCI | Any platform | Not supported |
| Cloud-specific (ECR/GAR/ACR) | Same platform | Same platform |

### What this means in practice

If your stack only has Container resources, you can use any OCI registry and deploy to any platform. Push to Docker Hub once, deploy everywhere.

If your stack has Function resources, you need a cloud-specific registry for each target platform. A Function targeting AWS requires ECR. A Function targeting GCP requires Artifact Registry.

A compile-time preflight validates this — if your stack has a Function resource and the images aren't hosted in the correct platform registry, the build fails before anything is deployed.

## Pushing Images

`alien release` pushes images to the project's configured artifact registry and creates a release record:

```bash
alien release --server http://server:8080
```

The CLI fetches the artifact registry configuration (type, endpoint, credentials) from alien-manager. The developer doesn't specify a repository — the server determines where images go based on the project's registry configuration.

Each resource's image is tagged and pushed as a sub-path under the registry's base URI.

## Cross-Account Registry Access

When a deployment's target environment is in a different cloud account than the artifact registry, the deployment loop grants that account pull access to the registry. This is common when deploying to a customer's cloud — images live in your registry, but the customer's compute resources need to pull them.

The `ArtifactRegistry` trait provides `add_cross_account_access()` and `remove_cross_account_access()` methods. The deployment loop calls these at two points:

1. **`initial-setup` → `provisioning` transition** — after frozen resources (IAM roles, service accounts) are created, the loop grants the target account pull access to the registry
2. **Deletion** — when a deployment reaches `deleted` status, the loop revokes access

Platform-specific mechanisms:

- **AWS (ECR):** Modifies the ECR repository policy to allow `ecr:GetDownloadUrlForLayer`, `ecr:BatchGetImage`, `ecr:BatchCheckLayerAvailability` for the target account. Account ID parsed from the management role ARN in stack state.
- **GCP (Artifact Registry):** Adds IAM bindings granting `roles/artifactregistry.reader` to the customer's project. Project number from `EnvironmentInfo` on the deployment record.
- **Azure / Kubernetes:** Not applicable — Azure and Kubernetes deployments don't use cross-account registry access.

For **same-account deployments** (target environment in the same account as the registry), no cross-account access is needed — the compute resources already have pull access.

For **generic OCI registries**, pull credentials are configured as part of the deployment's stack settings. The remote environment uses these credentials to authenticate with the registry at runtime.

For **local dev mode**, no pull credentials are needed — images load directly into Docker from local tarballs.

## Dev Mode

In dev mode (`alien dev`), no artifact registry is needed. `alien build` produces OCI tarballs in `.alien/build/`, and the deployment loop loads them directly into Docker via `ClientConfig::Local`. No push, no pull credentials, no registry.

## ReleaseStore

```rust
#[async_trait]
pub trait ReleaseStore: Send + Sync {
    async fn create_release(&self, stack: serde_json::Value) -> Result<ReleaseRecord>;
    async fn get_release(&self, id: &str) -> Result<Option<ReleaseRecord>>;
    async fn get_latest_release(&self) -> Result<Option<ReleaseRecord>>;
}
```

Default: `SqliteReleaseStore` — stores releases in the same SQLite database as other entities.

When a release is created, alien-manager sets `desired_release_id` on eligible deployments. See [Deployments — Desired vs Current Release](01-deployments.md#desired-vs-current-release).
