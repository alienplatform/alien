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

## AWS setup permission comparisons

Use a restricted installer to compare setup permissions before and after a
change. An installer that can rerun an existing stack has not necessarily been
qualified for a fresh install: resource creation, tagging, policy attachment,
and deletion can require additional grants.

1. Generate an unchanged baseline package from the base revision. Record the
   revision, binary hashes, template or package, installer policy, target
   account, region, and resource prefix. Use a small application with a Frozen
   bucket and a Live Worker that reads and writes that bucket.
2. Install the baseline with the restricted identity. For each access denial,
   save the full cloud error and add only the required action on the fixture's
   resources. Inspect the underlying denied action: CloudFormation's
   `UnauthorizedTaggingOperation` can wrap either `iam:CreateRole` or
   `iam:TagRole`. Record action and resource-scope additions separately.
3. After qualifying the policy, repeat from an absent stack and require
   `CREATE_COMPLETE`. A successful update after a failed create is useful
   diagnostic evidence, but does not complete the fresh-install check.
4. Rerun setup with the same baseline template, then publish a new application
   release. Require the deployment to reach Running and exercise a bucket write
   and read through the Worker. Preserve the registration identity during the
   rerun. Record any runtime failure separately from setup permission failures.
5. Install the changed package using the same qualified policy, changing only
   fixture names where necessary. Record every additional action or wider
   resource scope it requires. For permissions-boundary changes, inspect the
   installed boundary and management policies and test management restrictions
   using the installed management identity.
6. Delete each fixture through its setup tool with the same restricted
   installer. Record any cleanup grants separately and verify the roles,
   managed policies (including the boundary), and application resources are
   gone. Keep shared test infrastructure intact.

Repeat the comparison for CloudFormation, Terraform, and CLI setup wherever
that mode uses installer credentials. Report each mode separately; template
validation, policy simulation, and an automatic setup test do not establish
fresh-install or runtime-update success in another mode. Keep credentials,
registration tokens, and raw deployment evidence out of version control.
