import { AlienError } from "@alienplatform/core"
import { afterEach, describe, expect, it } from "vitest"
import { CommandsClient } from "../src/client.js"
import { createCommandReceiver } from "../src/receiver.js"
import type { StubServer } from "./helpers/stub-server.js"
import { encodeInlineJson, startStubServer } from "./helpers/stub-server.js"

let server: StubServer | undefined

afterEach(async () => {
  await server?.close()
  server = undefined
})

const FAST_POLL = { pollIntervalMs: 2, maxPollIntervalMs: 8 } as const

function bootstrapResponse(
  baseUrl: string,
  token: string,
  target?: { resourceId: string; resourceType: "container" | "daemon" },
  expiresAt = new Date(Date.now() + 5 * 60_000),
) {
  return {
    managerUrl: baseUrl,
    token,
    expiresAt: expiresAt.toISOString(),
    target,
  }
}

function createResponse() {
  return { commandId: "cmd_1", state: "PENDING", inlineAllowedUpTo: 150_000, next: "poll" }
}

function successStatus(value: unknown) {
  return {
    commandId: "cmd_1",
    state: "SUCCEEDED",
    attempt: 1,
    target: { resourceId: "agent", resourceType: "daemon" },
    response: {
      status: "success",
      response: { mode: "inline", inlineBase64: encodeInlineJson(value) },
    },
  }
}

describe("hosted command sender bootstrap", () => {
  it("discovers the manager and uses the minted sender token", async () => {
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        return { json: bootstrapResponse(server?.baseUrl ?? "", "sender-token") }
      }
      if (req.method === "POST" && req.path === "/v1/commands") {
        return { json: createResponse() }
      }
      return { json: successStatus({ ok: true }) }
    })

    const commands = await CommandsClient.forDeployment({
      deploymentId: "dep_1",
      apiKey: "api-key",
      platformUrl: server.baseUrl,
    })
    const result = await commands.invoke("ping", {}, FAST_POLL)

    expect(result).toEqual({ ok: true })
    expect(server.requests).toHaveLength(3)
    expect(server.requests[0]).toMatchObject({
      method: "POST",
      path: "/v1/commands/bootstrap",
      body: { deploymentId: "dep_1", role: "sender" },
    })
    expect(server.requests[0]?.headers.authorization).toBe("Bearer api-key")
    expect(server.requests[1]?.headers.authorization).toBe("Bearer sender-token")
    expect(server.requests[2]?.headers.authorization).toBe("Bearer sender-token")
  })

  it("refreshes bootstrap once and retries a manager request after 401", async () => {
    let bootstraps = 0
    let creates = 0
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        bootstraps += 1
        return { json: bootstrapResponse(server?.baseUrl ?? "", `sender-token-${bootstraps}`) }
      }
      if (req.method === "POST" && req.path === "/v1/commands") {
        creates += 1
        if (req.headers.authorization === "Bearer sender-token-1") {
          return { status: 401, text: "expired" }
        }
        return { json: createResponse() }
      }
      return { json: successStatus("done") }
    })

    const commands = await CommandsClient.forDeployment({
      deploymentId: "dep_1",
      apiKey: "api-key",
      platformUrl: server.baseUrl,
    })
    const result = await commands.invoke("ping", {}, FAST_POLL)

    expect(result).toBe("done")
    expect(bootstraps).toBe(2)
    expect(creates).toBe(2)
    expect(
      server.requests.filter(request => request.path === "/v1/commands/bootstrap")[1]?.body,
    ).toEqual({ deploymentId: "dep_1", role: "sender" })
  })

  it("refreshes a credential nearing expiry before the manager request", async () => {
    let bootstraps = 0
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        bootstraps += 1
        const expiresAt =
          bootstraps === 1 ? new Date(Date.now() + 1_000) : new Date(Date.now() + 5 * 60_000)
        return {
          json: bootstrapResponse(
            server?.baseUrl ?? "",
            `sender-token-${bootstraps}`,
            undefined,
            expiresAt,
          ),
        }
      }
      if (req.method === "POST" && req.path === "/v1/commands") {
        return { json: createResponse() }
      }
      return { json: successStatus("done") }
    })

    const commands = await CommandsClient.forDeployment({
      deploymentId: "dep_1",
      apiKey: "api-key",
      platformUrl: server.baseUrl,
    })
    await commands.invoke("ping", {}, FAST_POLL)

    expect(bootstraps).toBe(2)
    const managerRequests = server.requests.filter(
      request => request.path !== "/v1/commands/bootstrap",
    )
    expect(managerRequests).toHaveLength(2)
    expect(
      managerRequests.every(request => request.headers.authorization === "Bearer sender-token-2"),
    ).toBe(true)
  })
})

describe("hosted command receiver bootstrap", () => {
  it("defers discovery until run and leases for the inferred target", async () => {
    let stopReceiver = () => {}
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        return {
          json: bootstrapResponse(server?.baseUrl ?? "", "receiver-token", {
            resourceId: "agent",
            resourceType: "daemon",
          }),
        }
      }
      if (req.path === "/v1/commands/leases") {
        stopReceiver()
        return { json: { leases: [] } }
      }
      return { status: 404 }
    })

    const receiver = createCommandReceiver({
      deploymentId: "dep_1",
      apiKey: "api-key",
      target: "agent",
      platformUrl: server.baseUrl,
      pollIntervalMs: 1,
      pollJitter: 0,
    })
    stopReceiver = () => receiver.stop()
    expect(server.requests).toHaveLength(0)

    await receiver.run()

    expect(server.requests).toHaveLength(2)
    expect(server.requests[0]).toMatchObject({
      path: "/v1/commands/bootstrap",
      body: { deploymentId: "dep_1", role: "receiver", target: "agent" },
    })
    expect(server.requests[1]).toMatchObject({
      path: "/v1/commands/leases",
      body: {
        deploymentId: "dep_1",
        target: { resourceId: "agent", resourceType: "daemon" },
        maxLeases: 1,
        leaseSeconds: 60,
      },
    })
    expect(server.requests[1]?.headers.authorization).toBe("Bearer receiver-token")
  })

  it("refreshes bootstrap once and retries a lease request after 401", async () => {
    let stopReceiver = () => {}
    let bootstraps = 0
    let leases = 0
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        bootstraps += 1
        return {
          json: bootstrapResponse(server?.baseUrl ?? "", `receiver-token-${bootstraps}`, {
            resourceId: "agent",
            resourceType: "container",
          }),
        }
      }
      if (req.path === "/v1/commands/leases") {
        leases += 1
        if (req.headers.authorization === "Bearer receiver-token-1") {
          return { status: 401 }
        }
        stopReceiver()
        return { json: { leases: [] } }
      }
      return { status: 404 }
    })

    const receiver = createCommandReceiver({
      deploymentId: "dep_1",
      apiKey: "api-key",
      target: "agent",
      platformUrl: server.baseUrl,
      pollIntervalMs: 1,
      pollJitter: 0,
    })
    stopReceiver = () => receiver.stop()
    await receiver.run()

    expect(bootstraps).toBe(2)
    expect(leases).toBe(2)
    expect(server.requests.at(-1)?.headers.authorization).toBe("Bearer receiver-token-2")
  })

  it("retries a transient bootstrap failure inside the receiver backoff loop", async () => {
    let stopReceiver = () => {}
    let bootstraps = 0
    let leases = 0
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        bootstraps += 1
        if (bootstraps === 1) {
          return { status: 503, text: "temporarily unavailable" }
        }
        return {
          json: bootstrapResponse(server?.baseUrl ?? "", "receiver-token", {
            resourceId: "agent",
            resourceType: "container",
          }),
        }
      }
      if (req.path === "/v1/commands/leases") {
        leases += 1
        stopReceiver()
        return { json: { leases: [] } }
      }
      return { status: 404 }
    })

    const receiver = createCommandReceiver({
      deploymentId: "dep_1",
      apiKey: "api-key",
      target: "agent",
      platformUrl: server.baseUrl,
      pollIntervalMs: 1,
      pollJitter: 0,
    })
    stopReceiver = () => receiver.stop()
    expect(server.requests).toHaveLength(0)

    await receiver.run()

    expect(bootstraps).toBe(2)
    expect(leases).toBe(1)
    expect(server.requests.at(-1)?.headers.authorization).toBe("Bearer receiver-token")
  })

  it("rejects a receiver bootstrap response without an inferred target", async () => {
    server = await startStubServer(req => {
      if (req.path === "/v1/commands/bootstrap") {
        return { json: bootstrapResponse(server?.baseUrl ?? "", "receiver-token") }
      }
      return { status: 404 }
    })

    const receiver = createCommandReceiver({
      deploymentId: "dep_1",
      apiKey: "api-key",
      target: "agent",
      platformUrl: server.baseUrl,
    })
    const error = await receiver.run().catch((cause: unknown) => cause)

    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("MALFORMED_RESPONSE")
    expect(server.requests).toHaveLength(1)
  })
})
