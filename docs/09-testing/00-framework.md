# Testing Framework

`@alienplatform/testing` deploys Alien applications into real execution environments.

It intentionally has two modes:

- local mode uses `alien dev`
- cloud mode uses the platform API

This split is deliberate. Product-facing local testing should use the shipped CLI workflow; low-level infrastructure tests can still talk directly to standalone-manager helpers inside the repo.

## Local Mode

Local mode is the default.

```typescript
const deployment = await deploy({
  app: "./my-app",
})
```

What happens:

1. locate the `alien` binary from `ALIEN_CLI_PATH`, Cargo output, or `PATH`
2. allocate a free port
3. spawn `alien dev --port <N> --status-file <app>/.alien/testing-dev-status.json`
4. wait for the status file to report `ready`
5. read the primary agent/deployment from that file
6. return a `Deployment` handle using the reported public URL and commands URL

The local testing contract is the `DevStatus` JSON written by `alien dev`, not human-oriented terminal text.

## Cloud Mode

Cloud mode targets `aws`, `gcp`, or `azure`.

```typescript
const deployment = await deploy({
  app: "./my-app",
  platform: "aws",
})
```

What happens:

1. read `ALIEN_API_KEY`
2. run `alien build --platform <platform>`
3. create a release through the platform API
4. create a deployment through the platform API
5. poll until the deployment reaches `running`
6. return a `Deployment` handle

## Basic Usage

```typescript
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { deploy, type Deployment } from "@alienplatform/testing"

describe("My Alien App", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: "./my-app" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("responds to HTTP requests", async () => {
    const response = await fetch(`${deployment.url}/api/hello`)
    expect(response.status).toBe(200)
  })

  it("supports commands", async () => {
    const result = await deployment.invokeCommand("echo", { message: "hello" })
    expect(result.message).toBe("hello")
  })
})
```

## Deployment Handle

The returned `Deployment` object provides:

- `url` for HTTP access
- `invokeCommand(...)` for command execution
- `setExternalSecret(...)` for platform-native secret setup
- `upgrade(...)` for cloud-mode upgrades
- `destroy()` for cleanup

In local mode, `destroy()` terminates the spawned `alien dev` process. In cloud mode, it deletes the deployment through the platform API.

## Configuration

| Variable | Description | Default |
|---|---|---|
| `ALIEN_API_KEY` | Platform API key for cloud mode | — |
| `ALIEN_API_URL` | Platform API base URL for cloud mode | `https://api.alien.dev` |
| `ALIEN_CLI_PATH` | Path to the `alien` binary | auto-detected |
| `VERBOSE` | Forward child-process logs and extra progress | `false` |

## Why Local Mode Uses `alien dev`

This package is a product-facing abstraction. It should depend on the one CLI binary users already have, not on direct standalone-manager internals.

That gives local testing a simpler story:

- one binary to find
- one stable local contract
- the same local behavior users run manually

Repo-internal infrastructure tests can still use lower-level manager helpers directly when they need that control.

## Notes for CI

Local E2E keeps the one-binary story:

- build `alien`
- set `ALIEN_CLI_PATH`
- run tests

Cloud E2E additionally needs `ALIEN_API_KEY` and the relevant cloud credentials for the platform-managed provisioning flow.
