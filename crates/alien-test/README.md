# alien-test

E2E test harness for cross-cloud deployment testing. Provides reusable building blocks for tests that need a running alien-manager instance.

## Build Prerequisites

```bash
# Build binaries
cargo build -p alien-agent -p alien-deploy-cli

# Build runtime base image (x86_64 for GCP/Azure, arm64 for AWS Lambda)
docker buildx build -t alien-runtime:local --platform linux/amd64 \
  -f crates/alien-runtime/Dockerfile .
export ALIEN_TEST_OVERRIDE_BASE_IMAGE=alien-runtime:local

# Source test credentials
set -a && source .env.test && set +a
```

## Key Types

- **`TestManager`** — Starts a standalone alien-manager on a random port with temp SQLite. Provides authenticated SDK client.
- **`TestDeployment`** — Wraps a deployment with `deploy()`, `wait_until_running()`, `invoke_command()`, `upgrade()`, `destroy()`.
- **`TestAlienAgent`** — Manages agent lifecycle for pull-model tests (Docker, Helm, OS service).
- **`TestConfig`** — Loads cloud credentials from `.env.test`, reports available platforms.
- **`setup_target`** — Creates scoped IAM roles with auto-generated permissions for testing.

## Test Structure

Tests use `TestContext` with two roles:

1. **Developer** — Manager startup, build, release creation, deployment group + token
2. **Customer** — `alien-deploy up`, wait for running, run binding/command checks

| Platform | Model | Flow |
|---|---|---|
| AWS/GCP/Azure | Push | `alien-deploy up --platform <cloud>` |
| AWS/GCP/Azure | Pull | Docker alien-agent container |
| K8s | Pull | `helm install` |
| Local | Pull | `alien-deploy up --platform local` |

## Core Principle

E2E tests simulate real user flows. If a test needs manual cloud infrastructure setup, that signals the product is missing functionality.
