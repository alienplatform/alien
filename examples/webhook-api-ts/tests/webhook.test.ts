import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("webhook-api-ts", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("should respond to health check", async () => {
    const response = await fetch(`${deployment.url}/health`)
    expect(response.ok).toBe(true)

    const data = await response.json()
    expect(data.status).toBe("ok")
  })

  it("should receive and store a webhook", async () => {
    const response = await fetch(`${deployment.url}/webhooks/github`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action: "opened", number: 42 }),
    })
    expect(response.ok).toBe(true)

    const data = (await response.json()) as { received: boolean; key: string }
    expect(data.received).toBe(true)
    expect(data.key).toMatch(/^github:/)
  })

  it("should list recent events via HTTP", async () => {
    const response = await fetch(`${deployment.url}/webhooks/github/recent`)
    expect(response.ok).toBe(true)

    const data = (await response.json()) as { events: unknown[]; count: number }
    expect(data.count).toBeGreaterThan(0)
  })

  it("should query events via command", async () => {
    const result = await deployment.invokeCommand("get-events", { source: "github" })
    expect(result.count).toBeGreaterThan(0)
  })

  it("should track stats via command", async () => {
    const result = await deployment.invokeCommand("get-stats", { sources: ["github"] })
    expect(result.sources.github).toBeGreaterThan(0)
  })

  it("should isolate events by source", async () => {
    await fetch(`${deployment.url}/webhooks/stripe`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "payment_intent.succeeded" }),
    })

    const github = await deployment.invokeCommand("get-events", { source: "github" })
    const stripe = await deployment.invokeCommand("get-events", { source: "stripe" })

    expect(github.count).toBeGreaterThan(0)
    expect(stripe.count).toBeGreaterThan(0)

    // Events from different sources should not mix
    for (const e of github.events) {
      expect((e as { key: string }).key).toMatch(/^github:/)
    }
    for (const e of stripe.events) {
      expect((e as { key: string }).key).toMatch(/^stripe:/)
    }
  })
})
