/**
 * Sender unit tests against an in-test HTTP stub (no mocked fetch, no real
 * network). Every test drives the actual create → poll → decode machinery over
 * the platform global `fetch`, so the suite exercises exactly what production
 * would. Runs identically under Node (`vitest run`) and Bun (`bun test`).
 */

import { writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { AlienError } from "@alienplatform/core"
import { afterEach, describe, expect, it } from "vitest"
import { CommandsClient } from "../src/client.js"
import type { CapturedRequest, RouteResult, StubServer } from "./helpers/stub-server.js"
import { encodeInlineJson, startStubServer } from "./helpers/stub-server.js"

let server: StubServer | undefined

afterEach(async () => {
  await server?.close()
  server = undefined
})

/** Fast polling so the wall-clock loop resolves in milliseconds, not seconds. */
const FAST_POLL = { pollIntervalMs: 2, maxPollIntervalMs: 8 } as const

function client(baseUrl: string, allowLocalStorage = false) {
  return new CommandsClient({
    managerUrl: baseUrl,
    deploymentId: "dep_1",
    token: "tok_secret",
    allowLocalStorage,
  })
}

function createResponse(commandId = "cmd_1") {
  return { commandId, state: "PENDING", inlineAllowedUpTo: 150_000, next: "poll" }
}

function successStatus(commandId: string, value: unknown) {
  return {
    commandId,
    state: "SUCCEEDED",
    attempt: 1,
    target: { resourceId: "container-1", resourceType: "container" },
    response: {
      status: "success",
      response: { mode: "inline", inlineBase64: encodeInlineJson(value) },
    },
  }
}

describe("CommandsClient.invoke", () => {
  it("creates, polls, and decodes an inline success response", async () => {
    const returned = { report: "ok", rows: 3 }
    let polls = 0
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST" && req.path === "/v1/commands") {
        return { json: createResponse() }
      }
      // GET status: stay PENDING once, then succeed (proves the poll loop runs).
      polls += 1
      if (polls < 2) {
        return {
          json: {
            commandId: "cmd_1",
            state: "PENDING",
            attempt: 1,
            target: { resourceId: "container-1", resourceType: "container" },
          },
        }
      }
      return { json: successStatus("cmd_1", returned) }
    })

    const result = await client(server.baseUrl).invoke("generate-report", { a: 1 }, FAST_POLL)

    expect(result).toEqual(returned)
    expect(polls).toBeGreaterThanOrEqual(2)

    // create request shape + bearer auth
    const create = server.requests.find(r => r.method === "POST") as CapturedRequest
    expect(create.headers.authorization).toBe("Bearer tok_secret")
    const body = create.body as Record<string, unknown>
    expect(body.deploymentId).toBe("dep_1")
    expect(body.command).toBe("generate-report")
    // createBodySpec JSON-stringifies the input once, then base64s it.
    expect(body.params).toEqual({ mode: "inline", inlineBase64: encodeInlineJson({ a: 1 }) })
  })

  it("maps an error terminal state to DeploymentCommandError", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: {
          commandId: "cmd_1",
          state: "FAILED",
          attempt: 1,
          target: { resourceId: "container-1", resourceType: "container" },
          response: { status: "error", code: "BOOM", message: "handler blew up", details: "stack" },
        },
      }
    })

    const err = await client(server.baseUrl)
      .invoke("do-thing", {}, FAST_POLL)
      .catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alien = err as AlienError
    expect(alien.code).toBe("DEPLOYMENT_COMMAND_ERROR")
    expect(alien.context).toMatchObject({ errorCode: "BOOM", errorMessage: "handler blew up" })
  })

  it("maps an EXPIRED terminal state to CommandExpiredError", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: {
          commandId: "cmd_1",
          state: "EXPIRED",
          attempt: 1,
          target: { resourceId: "container-1", resourceType: "container" },
        },
      }
    })

    const err = await client(server.baseUrl)
      .invoke("do-thing", {}, FAST_POLL)
      .catch((e: unknown) => e)

    expect((err as AlienError).code).toBe("COMMAND_EXPIRED")
  })

  it("throws CommandTimeoutError (with lastState) when polling never terminates", async () => {
    let polls = 0
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      polls += 1
      return {
        json: {
          commandId: "cmd_1",
          state: "DISPATCHED",
          attempt: 1,
          target: { resourceId: "container-1", resourceType: "container" },
        },
      }
    })

    const err = await client(server.baseUrl)
      .invoke("slow", {}, { timeoutMs: 30, ...FAST_POLL })
      .catch((e: unknown) => e)

    const alien = err as AlienError
    expect(alien.code).toBe("COMMAND_TIMEOUT")
    expect(alien.context).toMatchObject({ timeoutMs: 30, lastState: "DISPATCHED" })
    // Multiple polls happened before the wall-clock timeout tripped (backoff loop ran).
    expect(polls).toBeGreaterThanOrEqual(2)
  })

  it("threads idempotencyKey and deadline into the create body", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return { json: successStatus("cmd_1", "done") }
    })

    const deadline = new Date("2030-01-01T00:00:00.000Z")
    await client(server.baseUrl).invoke(
      "x",
      {},
      { idempotencyKey: "idem-42", deadline, ...FAST_POLL },
    )

    const create = server.requests.find(r => r.method === "POST") as CapturedRequest
    const body = create.body as Record<string, unknown>
    expect(body.idempotencyKey).toBe("idem-42")
    expect(body.deadline).toBe("2030-01-01T00:00:00.000Z")
  })

  it("decodes a storage-mode (http backend) success response", async () => {
    const stored = { big: "payload", n: 7 }
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST" && req.path === "/v1/commands") return { json: createResponse() }
      if (req.method === "GET" && req.path === "/blob") return { text: JSON.stringify(stored) }
      // status → storage response pointing at /blob on this same server
      return {
        json: {
          commandId: "cmd_1",
          state: "SUCCEEDED",
          attempt: 1,
          target: { resourceId: "container-1", resourceType: "container" },
          response: {
            status: "success",
            response: {
              mode: "storage",
              size: 42,
              storageGetRequest: {
                backend: {
                  type: "http",
                  url: `${server?.baseUrl}/blob`,
                  method: "GET",
                  headers: {},
                },
                expiration: new Date(Date.now() + 60_000).toISOString(),
                operation: "get",
                path: "blob",
              },
            },
          },
        },
      }
    })

    const result = await client(server.baseUrl).invoke("fetch-big", {}, FAST_POLL)
    expect(result).toEqual(stored)
  })
})

describe("CommandsClient storage decode edge cases", () => {
  /** Build a status stub that answers with a storage-mode success body. */
  function storageStatus(bodyOverrides: Record<string, unknown>) {
    return {
      commandId: "cmd_1",
      state: "SUCCEEDED",
      attempt: 1,
      target: { resourceId: "container-1", resourceType: "container" },
      response: {
        status: "success",
        response: { mode: "storage", size: 10, ...bodyOverrides },
      },
    }
  }

  it("reads a local-backend storage response when allowLocalStorage is set", async () => {
    const stored = { local: true, n: 5 }
    const filePath = join(tmpdir(), `alien-cmd-decode-${Date.now()}.json`)
    await writeFile(filePath, JSON.stringify(stored), "utf-8")

    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: storageStatus({
          storageGetRequest: {
            backend: { type: "local", filePath, operation: "get" },
            expiration: new Date(Date.now() + 60_000).toISOString(),
            operation: "get",
            path: "local-blob",
          },
        }),
      }
    })

    const result = await client(server.baseUrl, true).invoke("read-local", {}, FAST_POLL)
    expect(result).toEqual(stored)
  })

  it("refuses the local backend when allowLocalStorage is false", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: storageStatus({
          storageGetRequest: {
            backend: { type: "local", filePath: "/tmp/whatever.json", operation: "get" },
            expiration: new Date(Date.now() + 60_000).toISOString(),
            operation: "get",
            path: "local-blob",
          },
        }),
      }
    })

    const err = await client(server.baseUrl, false)
      .invoke("read-local", {}, FAST_POLL)
      .catch((e: unknown) => e)
    expect((err as AlienError).code).toBe("STORAGE_OPERATION_FAILED")
    expect((err as AlienError).context).toMatchObject({
      reason: expect.stringContaining("not enabled"),
    })
  })

  it("rejects an expired presigned storage request", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: storageStatus({
          storageGetRequest: {
            backend: { type: "http", url: `${server?.baseUrl}/blob`, method: "GET", headers: {} },
            expiration: new Date(Date.now() - 1_000).toISOString(),
            operation: "get",
            path: "blob",
          },
        }),
      }
    })

    const err = await client(server.baseUrl)
      .invoke("fetch-expired", {}, FAST_POLL)
      .catch((e: unknown) => e)
    expect((err as AlienError).code).toBe("STORAGE_OPERATION_FAILED")
    expect((err as AlienError).context).toMatchObject({
      reason: expect.stringContaining("expired"),
    })
  })

  it("guards against path traversal in a local storage path", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return {
        json: storageStatus({
          storageGetRequest: {
            backend: { type: "local", filePath: "../../etc/passwd", operation: "get" },
            expiration: new Date(Date.now() + 60_000).toISOString(),
            operation: "get",
            path: "local-blob",
          },
        }),
      }
    })

    const err = await client(server.baseUrl, true)
      .invoke("traverse", {}, FAST_POLL)
      .catch((e: unknown) => e)
    expect((err as AlienError).code).toBe("STORAGE_OPERATION_FAILED")
    expect((err as AlienError).context).toMatchObject({
      reason: expect.stringContaining("Path traversal"),
    })
  })
})

describe("CommandsClient target threading", () => {
  it("sends targetResourceId from options.targetResourceId", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return { json: successStatus("cmd_1", "ok") }
    })

    await client(server.baseUrl).invoke("x", {}, { targetResourceId: "daemon-3", ...FAST_POLL })

    const create = server.requests.find(r => r.method === "POST") as CapturedRequest
    expect((create.body as Record<string, unknown>).targetResourceId).toBe("daemon-3")
  })

  it(".target(name) presets targetResourceId on the wire body", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return { json: successStatus("cmd_1", "ok") }
    })

    await client(server.baseUrl).target("container-9").invoke("x", {}, FAST_POLL)

    const create = server.requests.find(r => r.method === "POST") as CapturedRequest
    expect((create.body as Record<string, unknown>).targetResourceId).toBe("container-9")
  })

  it(".target(name) wins over a conflicting options.targetResourceId (builder wins)", async () => {
    server = await startStubServer((req): RouteResult => {
      if (req.method === "POST") return { json: createResponse() }
      return { json: successStatus("cmd_1", "ok") }
    })

    await client(server.baseUrl)
      .target("container-9")
      .invoke("x", {}, { targetResourceId: "container-other", ...FAST_POLL })

    const create = server.requests.find(r => r.method === "POST") as CapturedRequest
    expect((create.body as Record<string, unknown>).targetResourceId).toBe("container-9")
  })
})
