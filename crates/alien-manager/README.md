# alien-manager

The control plane for software you run in environments you don't control.

The manager stores releases and deployments, deploys push-mode environments
through cloud APIs, and serves pull-mode environments that connect to it
outbound: the Operator's sync endpoint, an OCI registry for images and Helm
charts, OpenTelemetry forwarding, tunnels into deployments, and remote commands.

## Running it

```bash
docker run -d -p 8080:8080 -v alien-data:/data \
  -e BASE_URL=https://manager.example.com \
  ghcr.io/alienplatform/alien-manager
```

- Kubernetes: the Helm chart in [`infra/helm/alien-manager`](../../infra/helm/alien-manager)
- Amazon ECS: the Terraform module in [`infra/aws-ecs-manager`](../../infra/aws-ecs-manager)
- Your machine: `alien serve`

State (database, keys, and release images unless an external registry is
configured) lives in `STATE_DIR`, `/data` in the image. The first start prints
an admin API key; set `ALIEN_ADMIN_TOKEN` to supply your own.

## Configuration

`alien-manager.toml`, read from `--config`, from the `ALIEN_MANAGER_CONFIG`
environment variable (the file's contents), or from the working directory.
`alien serve --init` writes a commented template. Common sections:

- `[artifact-registry.default]` stores release images in ECR, Artifact
  Registry or ACR instead of the manager's own registry
- `[telemetry]` forwards deployments' OpenTelemetry to your backend
- `[operator]` sets the Operator image deployments run and update to
- `[commands]` moves command state to DynamoDB, Firestore or Table Storage
- `[impersonation]` sets the identity used to deploy into customer clouds

## Embedding

The manager is a library. `AlienManagerBuilder` assembles it from store,
credential and authorization traits, so another service can embed it with its
own implementations. Tunnels are opt-in for embedders (`.tunnels()`); the
`alien-manager` binary and `alien serve` turn them on.
