# @alienplatform/testing

Testing framework for Alien applications.

## Install

```bash
npm install --save-dev @alienplatform/testing
```

## Local vs Cloud

This package has two deployment modes:

- local mode uses `alien dev`
- cloud mode uses the platform API

That split is intentional. Local mode stays centered on the shipped `alien` CLI binary instead of asking users to discover lower-level manager binaries.

## Local Mode

Local mode is the default:

```typescript
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { deploy, type Deployment } from "@alienplatform/testing"

describe("My App", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: "./my-app" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("responds to requests", async () => {
    const response = await fetch(`${deployment.url}/api/hello`)
    expect(response.status).toBe(200)
  })
})
```

Under the hood the package:

1. finds the `alien` binary
2. spawns `alien dev --status-file ...`
3. waits for the status file to report readiness
4. reads the public URL and commands URL from that file

The local machine contract is the `DevStatus` JSON written by `alien dev`, not terminal output.

## Cloud Mode

Cloud mode targets real infrastructure:

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
})
```

Supported cloud platforms:

- `aws`
- `gcp`
- `azure`

Cloud mode requires `ALIEN_API_KEY`.

## API

```typescript
interface DeployOptions {
  app: string
  platform?: "local" | "aws" | "gcp" | "azure"
  config?: string
  environmentVariables?: EnvironmentVariable[]
  verbose?: boolean
}
```

The returned `Deployment` supports:

- `deployment.url`
- `deployment.invokeCommand(name, params)`
- `deployment.setExternalSecret(vault, key, value)`
- `deployment.upgrade(options)` for cloud mode
- `deployment.destroy()`

## Configuration

| Variable | Description | Default |
|---|---|---|
| `ALIEN_API_KEY` | Platform API key for cloud mode | — |
| `ALIEN_API_URL` | Platform API base URL | `https://api.alien.dev` |
| `ALIEN_CLI_PATH` | Path to the `alien` binary | auto-detected |
| `VERBOSE` | Show detailed child-process logs | `false` |

## Notes

- local mode needs only the `alien` binary
- cloud mode needs `ALIEN_API_KEY`
- repo-internal low-level tests can still use standalone manager helpers directly; this package is the higher-level product-facing layer
