import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

type AcceptedTrace = {
  traceId: string
  contentHash: string
  status: "accepted"
}

type StoredTrace = {
  traceId: string
  agent: string
  status: string
  model: string
  payload: unknown
}

type TracePage = {
  traces: StoredTrace[]
  nextCursor?: string
}

async function waitForTrace(url: string, traceId: string): Promise<StoredTrace> {
  for (let attempt = 0; attempt < 30; attempt++) {
    const response = await fetch(`${url}/v1/traces/${traceId}`)
    if (response.ok) return response.json() as Promise<StoredTrace>
    expect([404, 503]).toContain(response.status)
    await new Promise(resolve => setTimeout(resolve, 500))
  }
  throw new Error(`trace '${traceId}' did not become visible`)
}

describe("SlateDB trace store", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })
  })

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("durably accepts, commits, reads, and filters a trace", async () => {
    const trace = {
      traceId: "integration-trace-1",
      agent: "researcher",
      status: "completed",
      model: "claude-sonnet",
      startedAt: "2026-08-26T18:00:00Z",
      finishedAt: "2026-08-26T18:00:04Z",
      payload: { answer: 42 },
    }
    const acceptedResponse = await fetch(`${deployment.url}/v1/traces`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(trace),
    })

    expect(acceptedResponse.status).toBe(202)
    const accepted = (await acceptedResponse.json()) as AcceptedTrace
    expect(accepted.traceId).toBe(trace.traceId)
    expect(accepted.status).toBe("accepted")
    expect(accepted.contentHash).toMatch(/^[a-f0-9]{64}$/)

    const stored = await waitForTrace(deployment.url!, trace.traceId)
    expect(stored).toMatchObject(trace)

    const query = new URLSearchParams({
      agent: trace.agent,
      status: trace.status,
      model: trace.model,
    })
    const listResponse = await fetch(`${deployment.url}/v1/traces?${query}`)
    expect(listResponse.ok).toBe(true)
    const page = (await listResponse.json()) as TracePage
    expect(page.traces.map(item => item.traceId)).toContain(trace.traceId)
  })

  it("treats an identical submission as idempotent", async () => {
    const trace = {
      traceId: "integration-trace-2",
      agent: "operator",
      status: "running",
      model: "claude-haiku",
      startedAt: "2026-08-26T19:00:00Z",
      payload: { step: 1 },
    }
    const submit = () =>
      fetch(`${deployment.url}/v1/traces`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(trace),
      })

    expect((await submit()).status).toBe(202)
    expect((await submit()).status).toBe(202)
    const stored = await waitForTrace(deployment.url!, trace.traceId)
    expect(stored.payload).toEqual(trace.payload)
  })

  it("rejects invalid traces before staging them", async () => {
    const response = await fetch(`${deployment.url}/v1/traces`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        traceId: "",
        agent: "researcher",
        status: "completed",
        model: "claude-sonnet",
        startedAt: "2026-08-26T18:00:00Z",
        payload: {},
      }),
    })

    expect(response.status).toBe(400)
    await expect(response.json()).resolves.toMatchObject({ code: "TRACE_INVALID" })
  })
})
