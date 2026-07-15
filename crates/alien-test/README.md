# alien-test

E2E test harness for cross-cloud deployment testing. Provides reusable building blocks for tests that need a running alien-manager instance.

## Build Prerequisites

```bash
# Build binaries
cargo build -p alien-operator -p alien-deploy-cli

# Source test credentials
set -a && source .env.test && set +a
```

Local E2E does not need a Worker base-image override. To test an unpublished
Worker runtime in cloud E2E, build both runtime architectures, push a
multi-architecture image to a registry reachable by the cloud builder, and set
`ALIEN_OVERRIDE_BASE_IMAGE` to that fully qualified image reference:

```bash
cargo zigbuild --release -p alien-worker-runtime --target x86_64-unknown-linux-musl
cargo zigbuild --release -p alien-worker-runtime --target aarch64-unknown-linux-musl
docker buildx build --push --platform linux/amd64,linux/arm64 \
  -t registry.example.com/alien-base:test \
  -f docker/Dockerfile.alien-base .
export ALIEN_OVERRIDE_BASE_IMAGE=registry.example.com/alien-base:test
```

The override applies to source-built Workers only. Source-built Containers and
Daemons use their direct base images.

## Key Types

- **`TestManager`** — Starts a standalone alien-manager on a random port with temp SQLite. Provides authenticated SDK client.
- **`TestDeployment`** — Wraps a deployment with `deploy()`, `wait_until_running()`, `invoke_command()`, `upgrade()`, `destroy()`.
- **`TestAlienOperator`** — Manages operator lifecycle for pull-model tests (Docker, Helm, OS service).
- **`TestConfig`** — Loads cloud credentials from `.env.test`, reports available platforms.
- **`setup_target`** — Creates scoped IAM roles with auto-generated permissions for testing.

## Test Structure

Tests use `TestContext` with two roles:

1. **Developer** — Manager startup, build, release creation, deployment group + token
2. **Customer** — `alien-deploy deploy`, wait for running, run binding/command checks

| Platform | Model | Flow |
|---|---|---|
| AWS/GCP/Azure | Push | `alien-deploy deploy --platform <cloud>` |
| AWS/GCP/Azure | Pull | Docker alien-operator container |
| K8s | Pull | `helm install` |
| Local | Pull | `alien-deploy deploy --platform local` |

## Core Principle

E2E tests simulate real user flows. If a test needs manual cloud infrastructure setup, that signals the product is missing functionality.
