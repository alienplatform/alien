# @alienplatform/testing

Testing framework for Alien applications. Deploy, test, and tear down Alien apps in local or cloud environments.

## Installation

```bash
npm install --save-dev @alienplatform/testing
```

## Quick Start

### Local Testing (default)

No credentials needed — uses `alien dev` under the hood:

```typescript
import { describe, it, expect, beforeAll, afterAll } from "vitest"
import { deploy, type Deployment } from "@alienplatform/testing"

describe("My App", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: "./my-app" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("should respond to requests", async () => {
    const response = await fetch(`${deployment.url}/api/hello`)
    expect(response.status).toBe(200)
  })

  it("should handle commands", async () => {
    const result = await deployment.invokeCommand("echo", { message: "hello" })
    expect(result.message).toBe("hello")
  })
})
```

### Cloud Testing

Requires `ALIEN_API_KEY` — deploys to real cloud infrastructure:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws", // or "gcp", "azure"
})
```

## Two Modes

| | Local | Cloud |
|---|---|---|
| **Trigger** | `platform: "local"` (default) | `platform: "aws" \| "gcp" \| "azure"` |
| **How** | Spawns `alien dev` | Builds + creates release/deployment via platform API |
| **Credentials** | None | `ALIEN_API_KEY` env var |
| **Speed** | ~5 seconds | 5-15 minutes |
| **Cleanup** | Kills process | DELETE via platform API |

## API Reference

### `deploy(options): Promise<Deployment>`

```typescript
interface DeployOptions {
  /** Path to application directory */
  app: string

  /** Target platform (default: "local") */
  platform?: "local" | "aws" | "gcp" | "azure"

  /** Specific config file (e.g., "alien.container.ts") */
  config?: string

  /** Environment variables for the deployment */
  environmentVariables?: EnvironmentVariable[]

  /** Show detailed logs */
  verbose?: boolean
}
```

### `Deployment`

```typescript
class Deployment {
  readonly id: string
  readonly name: string
  readonly url: string
  readonly platform: Platform
  destroyed: boolean

  /** Invoke a command on the deployment */
  async invokeCommand(name: string, params: any): Promise<any>

  /** Set an external secret using platform-native tools */
  async setExternalSecret(vault: string, key: string, value: string): Promise<void>

  /** Upgrade to a new release (cloud mode only) */
  async upgrade(options?: UpgradeOptions): Promise<void>

  /** Destroy the deployment and clean up resources */
  async destroy(): Promise<void>
}
```

## Environment Variables

Pass environment variables to your deployment:

```typescript
const deployment = await deploy({
  app: "./my-app",
  environmentVariables: [
    { name: "DATABASE_URL", value: "postgres://..." },
    { name: "API_KEY", value: "secret", type: "secret" },
    { name: "CACHE_TTL", value: "300", targetResources: ["my-function"] },
  ],
})
```

## External Secrets

Set platform-native secrets that deployments read via vault bindings:

```typescript
await deployment.setExternalSecret("my-vault", "API_KEY", "secret-value")
```

Uses AWS SSM Parameter Store, GCP Secret Manager, Azure Key Vault, or local vault depending on platform.

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ALIEN_API_KEY` | Platform API key (required for cloud mode) | — |
| `ALIEN_API_URL` | Platform API URL | `https://api.alien.dev` |
| `ALIEN_CLI_PATH` | Path to alien CLI binary | Auto-detected |
| `VERBOSE` | Show detailed logs | `false` |

## Best Practices

- **Always clean up** — destroy deployments in `afterAll`
- **Generous timeouts** — real deployments take minutes (use 300-600s for `beforeAll`)
- **Separate test accounts** — use dedicated cloud accounts for testing
- **Serial cloud tests** — cloud APIs have rate limits; run cloud tests serially

```typescript
// vitest.config.ts
export default {
  test: {
    testTimeout: 300_000,
    hookTimeout: 600_000,
  },
}
```

## License

ISC
