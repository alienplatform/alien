# OSS E2E Tests

End-to-end tests that deploy real applications to real infrastructure through a standalone alien-manager. No platform dependency — everything runs through OSS components.

## Test Matrix

Tests cover all combinations of language, compute type, and target environment:

|  | AWS | GCP | Azure | Kubernetes | Local |
|--|-----|-----|-------|------------|-------|
| **Rust function** | yes | yes | yes | yes | yes |
| **Rust container** | yes | yes | yes | yes | yes |
| **TypeScript function** | yes | yes | yes | yes | yes |
| **TypeScript container** | yes | yes | yes | yes | yes |
| **Python function** | planned | planned | planned | planned | planned |
| **Python container** | planned | planned | planned | planned | planned |

Each cell runs the same set of checks — health, bindings (storage, KV, vault, queue), commands, SSE, background tasks, environment variables, external secrets. If it passes on one combination, the binding/runtime behavior is correct. The matrix catches platform-specific and compute-type-specific issues.

## What OSS E2E Tests Validate

1. **Deployment lifecycle** — create → provision → running → update (new release) → redeploy → destroy
2. **Bindings** — Storage, KV, Vault, Queue, Artifact Registry, Build — against real cloud resources
3. **Commands** — push dispatch (Lambda invoke, Pub/Sub, Service Bus), pull polling, execute, respond
4. **Runtime features** — SSE, environment variables, background tasks (`wait_until`), readiness probes
5. **External secrets** — platform-native secret integration (SSM Parameter Store, GCP Secret Manager, Azure Key Vault)
6. **Telemetry** — OTLP log/trace/metric ingestion and forwarding
7. **Cross-account registry access** — grant/revoke pull access during deployment lifecycle
8. **Credential impersonation** — EnvironmentCredentialResolver with real cloud credentials
9. **Both compute types** — Function and Container deployments exercise different provisioning controllers
10. **Multiple languages** — Rust and TypeScript (Python planned) — verifies toolchain builds and runtime bindings per language

## What Remains Platform-Only

- CloudFormation, Terraform, Helm, Agent Image deployment methods (platform-generated artifacts)
- Multi-tenant workspace/project isolation
- Managed telemetry pipeline
- Dashboard/OAuth integration
- Platform manager (custom store and credential resolver implementations)

## Architecture

The test harness starts an alien-manager as a Docker container via Testcontainers. Tests create deployments through the server's API. The server's deployment loop provisions real cloud resources. Check functions then make HTTP requests to the deployed application to verify everything works.

```
┌─────────────────────────────────────────────────────┐
│                  Test Runner (Vitest)                │
│                                                     │
│  1. Start alien-manager container (Testcontainers)   │
│  2. Build test app (alien build --platform aws)     │
│  3. Create release, deployment group, deployment    │
│  4. Wait for "running"                              │
│  5. Run checks (HTTP → deployed app)                │
│  6. Cleanup (destroy deployment, stop container)    │
└──────────────────┬──────────────────────────────────┘
                   │
         ┌─────────┴─────────┐
         ▼                   ▼
   alien-manager         Cloud APIs
   (container)          (AWS/GCP/Azure)
         │                   ▲
         │  deployment loop  │
         └───────────────────┘
```

## Test Apps

Two comprehensive test applications, each available as both function and container:

### comprehensive-rust

Rust HTTP server that exposes endpoints for every binding and runtime feature:

```
test-apps/comprehensive-rust/
├── alien.function.ts    # Function deployment: all bindings
├── alien.container.ts   # Container deployment: all bindings
├── alien.dev.ts         # Local dev (no build/queue)
├── src/                        # Rust HTTP server
└── Cargo.toml
```

Both configs define the same resources — Storage, Vault, KV, Queue, Build, Artifact Registry — linked to a Function or Container compute resource with public ingress.

### comprehensive-typescript

TypeScript HTTP server (Hono) with the same endpoint structure:

```
test-apps/comprehensive-typescript/
├── alien.function.ts    # Function deployment
├── alien.container.ts   # Container deployment
├── src/index.ts                # Hono HTTP server
├── package.json
└── tsdown.config.ts
```

Same bindings, same checks, different language and toolchain. The TypeScript app uses `@alienplatform/bindings` for binding access and `hono` for HTTP routing.

### Test App Endpoint Convention

Both apps expose the same HTTP API so the same check functions work against either:

```
GET  /health                    → { status: "ok" }
GET  /hello                     → "Hello, ..."
POST /storage-test/{binding}    → storage read/write verification
POST /kv-test/{binding}         → KV read/write verification
POST /vault-test/{binding}      → vault read/write verification
POST /queue-test/{binding}      → queue send/receive verification
POST /artifact-registry-test/{binding} → push/pull verification
GET  /env-var/{name}            → environment variable value
GET  /sse                       → SSE event stream
POST /inspect                   → request echo
POST /wait-until-test           → background task trigger
GET  /events/list               → registered event handlers
```

## Test Structure

```
alien/tests/e2e/
├── test-apps/
│   ├── comprehensive-rust/
│   └── comprehensive-typescript/
├── checks/                      # Reusable verification functions
│   ├── index.ts                 # Re-exports all checks
│   ├── health.ts                # checkHealth(), checkHello()
│   ├── storage.ts               # checkStorage()
│   ├── kv.ts                    # checkKV()
│   ├── vault.ts                 # checkVault()
│   ├── queue.ts                 # checkQueue()
│   ├── build.ts                 # checkBuild()
│   ├── commands.ts              # checkCommandEcho(), checkCommandSmallPayload(), checkCommandLargePayload()
│   ├── sse.ts                   # checkSSE()
│   ├── environment.ts           # checkEnvironmentVariable()
│   ├── inspect.ts               # checkInspect()
│   ├── wait-until.ts            # checkWaitUntil()
│   ├── external-secrets.ts      # checkExternalSecret()
│   └── events.ts                # checkStorageEventHandler(), checkStorageEvent()
├── tests/
│   ├── rust-function/
│   │   ├── aws.test.ts
│   │   ├── gcp.test.ts
│   │   ├── azure.test.ts
│   │   ├── kubernetes.test.ts
│   │   └── local.test.ts
│   ├── rust-container/
│   │   ├── aws.test.ts
│   │   ├── gcp.test.ts
│   │   ├── azure.test.ts
│   │   ├── kubernetes.test.ts
│   │   └── local.test.ts
│   ├── typescript-function/
│   │   ├── aws.test.ts
│   │   ├── gcp.test.ts
│   │   ├── azure.test.ts
│   │   ├── kubernetes.test.ts
│   │   └── local.test.ts
│   ├── typescript-container/
│   │   ├── aws.test.ts
│   │   ├── gcp.test.ts
│   │   ├── azure.test.ts
│   │   ├── kubernetes.test.ts
│   │   └── local.test.ts
│   └── lifecycle.test.ts        # Update, delete, redeploy
├── harness/
│   ├── server.ts                # Start/stop alien-manager via Testcontainers
│   ├── config.ts                # E2E configuration from env vars
│   └── index.ts
└── vitest.config.ts
```

## Check Functions

Check functions are pure — they take a `Deployment` and verify one thing. They're shared across all test combinations (language, compute type, platform) and reusable by platform e2e tests.

```typescript
// checks/storage.ts
export async function checkStorage(deployment: Deployment): Promise<void> {
  const key = `test-${Date.now()}.txt`
  const value = 'hello from e2e'

  const writeRes = await fetch(`${deployment.url}/storage-test/test-alien-storage`, {
    method: 'POST',
    body: JSON.stringify({ operation: 'write', key, value }),
  })
  expect(writeRes.ok).toBe(true)

  const readRes = await fetch(`${deployment.url}/storage-test/test-alien-storage`, {
    method: 'POST',
    body: JSON.stringify({ operation: 'read', key }),
  })
  const data = await readRes.json()
  expect(data.value).toBe(value)
}
```

```typescript
// checks/commands.ts
export async function checkCommandEcho(deployment: Deployment): Promise<void> {
  const result = await deployment.invokeCommand('echo', { message: 'hello' })
  expect(result.message).toBe('hello')
}

export async function checkCommandLargePayload(deployment: Deployment): Promise<void> {
  // >48KB payload — forces storage-based response path
  const largeData = 'x'.repeat(50_000)
  const result = await deployment.invokeCommand('cmd-test-large', { data: largeData })
  expect(result.data).toBe(largeData)
}
```

## Test Anatomy

Every test file follows the same pattern. Only the app path, config file, and platform change:

```typescript
import { deploy, type Deployment } from '@alienplatform/testing'
import * as checks from '../../checks/index.js'

describe('Rust function - AWS', () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({
      app: '../../test-apps/comprehensive-rust',
      config: 'alien.function.ts',
      platform: 'aws',
    })
  }, 900_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it('health check', () => checks.checkHealth(deployment))
  it('hello endpoint', () => checks.checkHello(deployment))
  it('storage binding', () => checks.checkStorage(deployment))
  it('kv binding', () => checks.checkKV(deployment))
  it('vault binding', () => checks.checkVault(deployment))
  it('external secrets', () => checks.checkExternalSecret(deployment), 30_000)
  it('artifact registry', () => checks.checkArtifactRegistry(deployment))
  it('queue binding', () => checks.checkQueue(deployment))
  it('build binding', () => checks.checkBuild(deployment), 600_000)
  it('SSE', () => checks.checkSSE(deployment))
  it('environment variables', () => checks.checkEnvironmentVariable(deployment))
  it('request inspection', () => checks.checkInspect(deployment))
  it('wait_until background tasks', () => checks.checkWaitUntil(deployment), 60_000)
  it('command echo', () => checks.checkCommandEcho(deployment))
  it('command small payload', () => checks.checkCommandSmallPayload(deployment))
  it('command large payload', () => checks.checkCommandLargePayload(deployment))
  it('event handlers registered', () => checks.checkStorageEventHandler(deployment))
})
```

A container test looks identical — just swap the config:

```typescript
deployment = await deploy({
  app: '../../test-apps/comprehensive-rust',
  config: 'alien.container.ts',  // ← only difference
  platform: 'aws',
})
```

## Configuration

### Required

```bash
# Cloud credentials (for the platform being tested)
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
AWS_REGION=us-east-1
```

The alien-manager container is started automatically by Testcontainers. No `ALIEN_MANAGER_URL` or `ALIEN_API_KEY` needed — the framework handles it.

### Optional

```bash
ALIEN_MANAGER_URL=http://localhost:8080     # Skip Testcontainers, use existing server
ALIEN_API_KEY=ax_admin_...            # Required when using ALIEN_MANAGER_URL
SKIP_CLEANUP=true                     # Keep resources for debugging
VERBOSE=true                          # Detailed logs
```

### GCP

```bash
GOOGLE_SERVICE_ACCOUNT_KEY='{"type":"service_account",...}'
GOOGLE_REGION=europe-central2
```

### Azure

```bash
AZURE_SUBSCRIPTION_ID=...
AZURE_TENANT_ID=...
AZURE_CLIENT_ID=...
AZURE_CLIENT_SECRET=...
AZURE_REGION=eastus
```

### Kubernetes

```bash
KUBECONFIG=/path/to/kubeconfig  # defaults to ~/.kube/config
```

### Local

No credentials needed — resources are provisioned with Docker on the host machine.

## Running Tests

```bash
cd alien

# All tests
pnpm test:e2e

# Specific language + compute type
pnpm test:e2e tests/rust-function/
pnpm test:e2e tests/typescript-container/

# Specific platform
pnpm test:e2e tests/rust-function/aws.test.ts

# Local only (no cloud credentials needed)
pnpm test:e2e tests/rust-function/local.test.ts
pnpm test:e2e tests/typescript-function/local.test.ts

# Debug a failure
SKIP_CLEANUP=true VERBOSE=true pnpm test:e2e tests/rust-function/aws.test.ts
```

## Vitest Configuration

```typescript
// vitest.config.ts
export default {
  test: {
    include: ['tests/**/*.test.ts'],
    pool: 'forks',
    poolOptions: { forks: { singleFork: true } }, // serial — cloud rate limits
    testTimeout: 600_000,   // 10 min per test
    hookTimeout: 900_000,   // 15 min for setup/teardown
  },
}
```

## CI/CD

GitHub Actions with matrix strategy for language × compute × platform:

```yaml
name: E2E Tests

on:
  push:
    branches: [main]
  pull_request:

jobs:
  e2e:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        suite:
          - rust-function
          - rust-container
          - typescript-function
          - typescript-container
        platform: [aws, gcp, azure, kubernetes]
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v1
      - uses: dtolnay/rust-toolchain@stable

      - name: Install dependencies
        run: cd alien && bun install

      - name: Run E2E tests
        run: cd alien && pnpm test:e2e tests/${{ matrix.suite }}/${{ matrix.platform }}.test.ts
        env:
          AWS_ACCESS_KEY_ID: ${{ secrets.AWS_ACCESS_KEY_ID }}
          AWS_SECRET_ACCESS_KEY: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
          AWS_REGION: us-east-1
          GOOGLE_SERVICE_ACCOUNT_KEY: ${{ secrets.GOOGLE_SERVICE_ACCOUNT_KEY }}
          GOOGLE_REGION: europe-central2
          AZURE_SUBSCRIPTION_ID: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
          AZURE_TENANT_ID: ${{ secrets.AZURE_TENANT_ID }}
          AZURE_CLIENT_ID: ${{ secrets.AZURE_CLIENT_ID }}
          AZURE_CLIENT_SECRET: ${{ secrets.AZURE_CLIENT_SECRET }}
          AZURE_REGION: eastus
          KUBECONFIG: ${{ secrets.KUBECONFIG }}

  e2e-local:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        suite: [rust-function, typescript-function]
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v1
      - uses: dtolnay/rust-toolchain@stable

      - name: Install dependencies
        run: cd alien && bun install

      - name: Run local E2E tests
        run: cd alien && pnpm test:e2e tests/${{ matrix.suite }}/local.test.ts
```

The `e2e-local` job requires no cloud credentials and runs on every PR. Cloud jobs can be gated on labels or run on a schedule.
