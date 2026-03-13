/**
 * GitHub Agent Remote - Integration Tests
 *
 * Uses @aliendotdev/testing with the dev deployer for pure local command testing.
 */

import { afterAll, beforeAll, describe, expect, it } from "vitest"
import { deploy, type Deployment } from "@aliendotdev/testing"

const integrationId = "demo-repo"

const demoConfig = {
  owner: "acme-corp",
  repo: "api",
  token: "demo",
}

describe("GitHub Agent Remote", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({
      app: ".",
      platform: "local",
    })

    await deployment.invokeCommand("set-integration", {
      integrationId,
      config: demoConfig,
    })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("serves a health endpoint", async () => {
    const response = await fetch(`${deployment.url}/health`)

    expect(response.ok).toBe(true)

    const data = await response.json() as { status: string; timestamp: string }
    expect(data.status).toBe("ok")
    expect(data.timestamp).toBeDefined()
  })

  it("analyzes repository metrics via command invocation", async () => {
    const result = await deployment.invokeCommand("analyze-repository", { integrationId })

    expect(result.totalPRs).toBe(6)
    expect(result.bySize.small).toBe(3)
    expect(result.bySize.medium).toBe(2)
    expect(result.bySize.large).toBe(1)
    expect(result.churnHotspots.length).toBeGreaterThan(0)
  })

  it("labels PRs in demo mode", async () => {
    const result = await deployment.invokeCommand("label-pull-requests", { integrationId })

    expect(result.labeled).toBe(6)
    expect(result.labels).toContain("size:small")
  })

  it("returns pull requests directly over HTTP", async () => {
    const response = await fetch(
      `${deployment.url}/prs?integrationId=${encodeURIComponent(integrationId)}`,
    )

    expect(response.ok).toBe(true)

    const data = await response.json() as {
      integrationId: string
      pullRequests: Array<{ size: string; risk: string; aiReview: unknown }>
    }
    expect(data.integrationId).toBe(integrationId)
    expect(data.pullRequests.length).toBe(6)
    expect(data.pullRequests[0].size).toBeDefined()
    expect(data.pullRequests[0].risk).toBeDefined()
  })
})
