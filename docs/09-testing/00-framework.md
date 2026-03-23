# Testing Framework

`@alienplatform/testing` deploys applications to real environments — AWS, GCP, Azure, Kubernetes, or locally with Docker. No mocks, no simulators — test deployments are regular Alien deployments. The same provisioning engine, the same runtime, the same infrastructure.

## How It Works

The testing framework starts an alien-manager as a Docker container via [Testcontainers](https://testcontainers.com/), then uses the server's API to build, release, and deploy your application to real cloud infrastructure. The server's deployment loop handles provisioning — same code path as production.

```
┌────────────────────────────────────────────────────┐
│                 Test Runner (Vitest)                │
│                                                    │
│  1. Start alien-manager container (Testcontainers)  │
│  2. Build app → create release                     │
│  3. Create deployment group + deployment           │
│  4. Server's deployment loop provisions resources  │
│  5. Poll until "running"                           │
│  6. Run checks (HTTP → deployed app)               │
│  7. Cleanup                                        │
└──────────────────┬─────────────────────────────────┘
                   │
         ┌─────────┴─────────┐
         ▼                   ▼
   alien-manager         Cloud APIs
   (container)          (AWS/GCP/Azure)
         │                   ▲
         │  deployment loop  │
         └───────────────────┘
```

No workspace, no project, no deployment method selection. The framework manages the server lifecycle — you just provide your app and cloud credentials.

## Installation

```bash
npm install --save-dev @alienplatform/testing
```

## Basic Usage

```typescript
import { describe, it, expect, beforeAll, afterAll } from 'vitest'
import { deploy, type Deployment } from '@alienplatform/testing'

describe('My Alien App', () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({
      app: './my-app',
      platform: 'aws',
    })
  }, 600_000) // 10 min — real cloud provisioning is slow

  afterAll(async () => {
    await deployment?.destroy()
  })

  it('should respond to requests', async () => {
    expect(deployment.status).toBe('running')

    const response = await fetch(`${deployment.url}/api/hello`)
    expect(response.status).toBe(200)
  })
})
```

That's it. The framework:

1. Starts an alien-manager container (or reuses an existing one if `ALIEN_SERVER` is set)
2. Runs `alien build --platform aws` on your app
3. Creates a release via the server API
4. Creates a deployment group and deployment
5. The server's deployment loop provisions real resources using your credentials (from environment variables)
6. Polls until the deployment reaches `running` status
7. Returns a `Deployment` handle with the deployed app's URL

For local deployments (`platform: 'local'`), no cloud credentials are needed — resources are provisioned with Docker on the host machine.

## Credentials

Credentials come from standard environment variables. The alien-manager's `EnvironmentCredentialResolver` picks them up automatically.

```bash
# AWS
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=us-east-1

# GCP
export GOOGLE_SERVICE_ACCOUNT_KEY='{"type":"service_account",...}'
export GOOGLE_REGION=europe-central2

# Azure
export AZURE_SUBSCRIPTION_ID=...
export AZURE_TENANT_ID=...
export AZURE_CLIENT_ID=...
export AZURE_CLIENT_SECRET=...
export AZURE_REGION=eastus

# Kubernetes
export KUBECONFIG=/path/to/kubeconfig  # defaults to ~/.kube/config

# Local — no credentials needed
```

Or pass them explicitly:

```typescript
const deployment = await deploy({
  app: './my-app',
  platform: 'aws',
  credentials: {
    platform: 'aws',
    accessKeyId: '...',
    secretAccessKey: '...',
    region: 'us-east-1',
  },
})
```

## Deploy Options

```typescript
interface DeployOptions {
  /** Path to application directory */
  app: string

  /** Target platform: 'aws' | 'gcp' | 'azure' | 'kubernetes' | 'local' */
  platform: Platform

  /** Specific config file (e.g., 'alien.container.ts') */
  config?: string

  /** Environment variables for the deployment */
  environmentVariables?: EnvironmentVariable[]

  /** Stack settings overrides */
  stackSettings?: Partial<StackSettings>

  /** Explicit cloud credentials (otherwise uses env vars) */
  credentials?: PlatformCredentials

  /** Show detailed logs */
  verbose?: boolean
}
```

The `config` option lets you test different deployment shapes with the same app:

```typescript
// Test as a function
const fnDeployment = await deploy({
  app: './test-apps/comprehensive',
  config: 'alien.function.ts',
  platform: 'aws',
})

// Test as a container
const containerDeployment = await deploy({
  app: './test-apps/comprehensive',
  config: 'alien.container.ts',
  platform: 'aws',
})
```

## Deployment API

The `Deployment` class returned by `deploy()`:

```typescript
class Deployment {
  readonly id: string
  readonly name: string
  readonly url: string         // Public URL of the deployed app
  readonly platform: Platform
  readonly resourcePrefix: string
  readonly status: DeploymentStatus

  /** Refresh deployment info from server */
  async refresh(): Promise<void>

  /** Wait for a specific status (with configurable timeout) */
  async waitForStatus(status: DeploymentStatus, opts?: WaitOptions): Promise<void>

  /** Invoke a remote command on the deployment */
  async invokeCommand(name: string, params: any): Promise<any>

  /** Set a secret in the deployment's vault via platform-native tools */
  async setExternalSecret(vault: string, key: string, value: string): Promise<void>

  /** Destroy the deployment and clean up cloud resources */
  async destroy(): Promise<void>
}
```

### Commands

```typescript
const result = await deployment.invokeCommand('echo', { message: 'hello' })
expect(result.message).toBe('hello')
```

Uses the command server — push dispatch (Lambda invoke, Pub/Sub, Service Bus) or pull polling, depending on the platform.

### External Secrets

```typescript
await deployment.setExternalSecret('my-vault', 'API_KEY', 'secret-value')
```

Writes secrets using platform-native tools (AWS SSM Parameter Store, GCP Secret Manager, Azure Key Vault) so the deployment can read them via vault bindings.

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ALIEN_SERVER` | Skip Testcontainers, use existing server | — (starts container) |
| `ALIEN_API_KEY` | API key (when using existing server) | — (auto-generated for container) |
| `VERBOSE` | Show detailed logs | `false` |
| `SKIP_CLEANUP` | Keep resources after tests for debugging | `false` |

When `ALIEN_SERVER` is set, the framework skips starting a container and connects to the existing server. Useful for development — run alien-manager locally and iterate on tests without container startup overhead.

## Best Practices

**Separate test accounts.** Always use dedicated cloud accounts for testing. Never use production credentials.

**Always clean up.** Destroy deployments in `afterAll`. If a test crashes, clean up manually via the server API or cloud console.

**Generous timeouts.** Real deployments take minutes. Use 600s (10 min) for `beforeAll`, longer for builds.

**Serial execution.** Cloud APIs have rate limits and resource quotas. Run cloud tests serially:

```typescript
// vitest.config.ts
export default {
  test: {
    pool: 'forks',
    poolOptions: { forks: { singleFork: true } },
    testTimeout: 600_000,
    hookTimeout: 900_000,
  },
}
```
