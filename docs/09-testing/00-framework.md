# Testing Framework

`@alienplatform/testing` deploys applications to real environments — AWS, GCP, Azure, or locally with Docker. No mocks, no simulators. The same provisioning engine, the same runtime, the same infrastructure.

## Two Modes

The framework auto-detects the deployment method based on the target platform.

### Local Mode (default)

Spawns `alien dev` as a child process. No credentials, no cloud accounts — everything runs locally.

```typescript
const deployment = await deploy({
  app: './my-app',
  // platform defaults to "local"
})
```

What happens:
1. Finds the `alien` CLI (from `ALIEN_CLI_PATH`, Cargo build output, or PATH)
2. Allocates a free port
3. Spawns `alien dev --no-tui --port <N>`
4. Polls `/health` until the server is ready
5. Polls `/v1/deployments` until the deployment reaches `running`
6. Returns a `Deployment` handle with the public URL

### Cloud Mode

Builds the app and deploys via the platform API. Requires `ALIEN_API_KEY`.

```typescript
const deployment = await deploy({
  app: './my-app',
  platform: 'aws', // or 'gcp', 'azure'
})
```

What happens:
1. Reads `ALIEN_API_KEY` from environment (required)
2. Runs `alien build --platform <aws|gcp|azure>`
3. Creates a release via `POST /v1/releases`
4. Creates a deployment via `POST /v1/deployments`
5. Polls until the deployment reaches `running` (up to 15 minutes)
6. Returns a `Deployment` handle

## Basic Usage

```typescript
import { describe, it, expect, beforeAll, afterAll } from 'vitest'
import { deploy, type Deployment } from '@alienplatform/testing'

describe('My Alien App', () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: './my-app' })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it('should respond to requests', async () => {
    const response = await fetch(`${deployment.url}/api/hello`)
    expect(response.status).toBe(200)
  })

  it('should handle commands', async () => {
    const result = await deployment.invokeCommand('echo', { message: 'hello' })
    expect(result.message).toBe('hello')
  })
})
```

## Deploy Options

```typescript
interface DeployOptions {
  /** Path to application directory */
  app: string

  /** Target platform (default: 'local') */
  platform?: 'local' | 'aws' | 'gcp' | 'azure'

  /** Specific config file (e.g., 'alien.container.ts') */
  config?: string

  /** Environment variables for the deployment */
  environmentVariables?: EnvironmentVariable[]

  /** Show detailed logs */
  verbose?: boolean
}
```

The `config` option lets you test different deployment shapes:

```typescript
// Test as a function
const fn = await deploy({ app: './my-app', config: 'alien.function.ts', platform: 'aws' })

// Test as a container
const container = await deploy({ app: './my-app', config: 'alien.container.ts', platform: 'aws' })
```

## Deployment API

```typescript
class Deployment {
  readonly id: string        // Deployment ID
  readonly name: string      // Deployment name
  readonly url: string       // Public URL of the deployed app
  readonly platform: Platform
  destroyed: boolean

  /** Invoke a command on the deployment */
  async invokeCommand(name: string, params: any): Promise<any>

  /** Set a secret using platform-native tools (SSM, Secret Manager, Key Vault) */
  async setExternalSecret(vault: string, key: string, value: string): Promise<void>

  /** Push a new release and wait for it to be active (cloud mode only) */
  async upgrade(options?: UpgradeOptions): Promise<void>

  /** Destroy the deployment and clean up resources */
  async destroy(): Promise<void>
}
```

### Commands

```typescript
const result = await deployment.invokeCommand('echo', { message: 'hello' })
expect(result.message).toBe('hello')
```

### External Secrets

```typescript
await deployment.setExternalSecret('my-vault', 'API_KEY', 'secret-value')
```

Writes secrets using platform-native tools — AWS SSM Parameter Store, GCP Secret Manager, Azure Key Vault, or the local vault CLI — so the deployment reads them via vault bindings.

### Upgrade (Cloud Only)

```typescript
// Modify your app, then:
await deployment.upgrade()

// With new env vars:
await deployment.upgrade({
  environmentVariables: [{ name: 'NEW_VAR', value: 'hello' }],
})
```

Rebuilds the app, creates a new release, updates the deployment, and waits for the new release to be running.

## Credentials

Cloud mode reads `ALIEN_API_KEY` from the environment:

```bash
export ALIEN_API_KEY=your-key-here
```

Cloud provider credentials come from standard environment variables — the platform handles provisioning.

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ALIEN_API_KEY` | Platform API key (required for cloud mode) | — |
| `ALIEN_API_URL` | Platform API URL | `https://api.alien.dev` |
| `ALIEN_CLI_PATH` | Path to alien CLI binary | Auto-detected |
| `VERBOSE` | Show detailed logs | `false` |

## Best Practices

**Always clean up.** Destroy deployments in `afterAll`. If a test crashes, clean up manually.

**Generous timeouts.** Local deploys take seconds, cloud deploys take minutes. Use 300s+ for `beforeAll`.

**Separate test accounts.** Never use production cloud accounts for testing.

**Serial cloud tests.** Cloud APIs have rate limits. Run cloud tests serially:

```typescript
// vitest.config.ts
export default {
  test: {
    testTimeout: 300_000,
    hookTimeout: 600_000,
  },
}
```
