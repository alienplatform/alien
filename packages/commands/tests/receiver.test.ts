/**
 * Pull receiver unit tests against the in-test HTTP stub (no mocked fetch, no
 * real network). Every test drives the actual lease → decode → handle → submit
 * machinery over the platform global `fetch`, so the suite exercises exactly
 * what production would. Runs identically under Node (`vitest run`) and Bun
 * (`bun test`).
 */

import { AlienError } from "@alienplatform/core"
import { afterEach, describe, expect, it } from "vitest"
import type { CommandResponse, Envelope, LeaseInfo } from "../src/protocol.js"
import { createCommandReceiver } from "../src/receiver.js"
import type { CommandContext } from "../src/receiver.js"
import type {
  CapturedRequest,
  RouteHandler,
  RouteResult,
  StubServer,
} from "./helpers/stub-server.js"
import { encodeInlineJson, startStubServer } from "./helpers/stub-server.js"

let server: StubServer | undefined
let receiverStop: (() => void) | undefined
let running: Promise<void> | undefined
// Reassignable route so the stub keeps its port while we bind the envelope
// (which needs the base url) after the server is already listening.
let route: RouteHandler = () => ({ status: 404 })

afterEach(async () => {
  receiverStop?.()
  await running?.catch(() => {})
  await server?.close()
  server = undefined
  receiverStop = undefined
  running = undefined
  route = () => ({ status: 404 })
})

/** Start the stub once; its port never changes. Set the real route afterwards. */
async function openServer(): Promise<StubServer> {
  const s = await startStubServer(req => route(req))
  server = s
  return s
}

async function waitFor(predicate: () => boolean, timeoutMs = 2000): Promise<void> {
  const start = Date.now()
  while (!predicate()) {
    if (Date.now() - start > timeoutMs) throw new Error("waitFor timed out")
    await new Promise(r => setTimeout(r, 2))
  }
}

const FULL_ENV = {
  ALIEN_COMMANDS_URL: "http://127.0.0.1:1/v1/",
  ALIEN_COMMANDS_TOKEN: "tok",
  ALIEN_DEPLOYMENT_ID: "dep-123",
  ALIEN_COMMANDS_TARGET_RESOURCE_ID: "agent",
  ALIEN_COMMANDS_TARGET_RESOURCE_TYPE: "daemon",
} as const

function inlineParams(value: unknown) {
  return { mode: "inline" as const, inlineBase64: encodeInlineJson(value) }
}

function envelope(overrides: Partial<Envelope> & { baseUrl: string }): Envelope {
  const { baseUrl, ...rest } = overrides
  return {
    protocol: "arc.v1",
    deploymentId: "dep-123",
    target: { resourceId: "agent", resourceType: "daemon" },
    commandId: "cmd_1",
    attempt: 1,
    command: "echo",
    params: inlineParams({ key: "value" }),
    responseHandling: {
      maxInlineBytes: 150_000,
      submitResponseUrl: `${baseUrl}/v1/commands/cmd_1/response`,
      storageUploadRequest: {
        backend: { type: "http", url: `${baseUrl}/storage-put`, method: "PUT", headers: {} },
        expiration: new Date(Date.now() + 3_600_000).toISOString(),
        operation: "put",
        path: "resp-path",
      },
    },
    ...rest,
  }
}

function lease(env: Envelope, over: Partial<LeaseInfo> = {}): LeaseInfo {
  return {
    leaseId: "lease_1",
    leaseExpiresAt: new Date(Date.now() + 60_000).toISOString(),
    commandId: env.commandId,
    attempt: env.attempt,
    envelope: env,
    ...over,
  }
}

/** Serve a single lease batch on the first poll, empty batches afterwards. */
function leaseOnce(leases: LeaseInfo[]): (req: CapturedRequest) => RouteResult | undefined {
  let served = false
  return req => {
    if (req.method === "POST" && req.path === "/v1/commands/leases") {
      if (served) return { json: { leases: [] } }
      served = true
      return { json: { leases } }
    }
    return undefined
  }
}

/** Find the submit PUT (a CommandResponse body) for a command id. */
function submitBody(id: string): CommandResponse | undefined {
  const put = server?.requests.find(
    r => r.method === "PUT" && r.path === `/v1/commands/${id}/response`,
  )
  return put?.body as CommandResponse | undefined
}

function decodeInline(response: Extract<CommandResponse, { status: "success" }>): unknown {
  if (response.response.mode !== "inline") throw new Error("expected inline body")
  return JSON.parse(Buffer.from(response.response.inlineBase64, "base64").toString("utf-8"))
}

// ---------------------------------------------------------------------------
// Environment validation (sync-throwing; no server)
// ---------------------------------------------------------------------------

describe("createCommandReceiver env validation", () => {
  it("accepts a valid container config", () => {
    expect(() =>
      createCommandReceiver({
        env: { ...FULL_ENV, ALIEN_COMMANDS_TARGET_RESOURCE_TYPE: "container" },
      }),
    ).not.toThrow()
  })

  it("accepts a valid daemon config", () => {
    expect(() => createCommandReceiver({ env: { ...FULL_ENV } })).not.toThrow()
  })

  for (const missing of Object.keys(FULL_ENV)) {
    it(`fails fast naming ${missing} when it is missing`, () => {
      const env: Record<string, string> = { ...FULL_ENV }
      delete env[missing]
      let err: unknown
      try {
        createCommandReceiver({ env })
      } catch (e) {
        err = e
      }
      expect(err).toBeInstanceOf(AlienError)
      const alien = err as AlienError
      expect(alien.code).toBe("COMMAND_RECEIVER_CONFIG_INVALID")
      expect(alien.context).toMatchObject({ envVar: missing })
    })
  }

  it("rejects an empty string value (fixture path)", () => {
    let err: unknown
    try {
      createCommandReceiver({ env: { ...FULL_ENV, ALIEN_COMMANDS_URL: "" } })
    } catch (e) {
      err = e
    }
    expect((err as AlienError).code).toBe("COMMAND_RECEIVER_CONFIG_INVALID")
    expect((err as AlienError).context).toMatchObject({ envVar: "ALIEN_COMMANDS_URL" })
  })

  it("rejects the worker target type", () => {
    let err: unknown
    try {
      createCommandReceiver({ env: { ...FULL_ENV, ALIEN_COMMANDS_TARGET_RESOURCE_TYPE: "worker" } })
    } catch (e) {
      err = e
    }
    const alien = err as AlienError
    expect(alien.code).toBe("COMMAND_RECEIVER_CONFIG_INVALID")
    expect(alien.context).toMatchObject({ envVar: "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE" })
    expect(alien.message).toContain("container")
    expect(alien.message).toContain("daemon")
  })

  it("rejects an unparseable URL", () => {
    let err: unknown
    try {
      createCommandReceiver({ env: { ...FULL_ENV, ALIEN_COMMANDS_URL: "not a url" } })
    } catch (e) {
      err = e
    }
    expect((err as AlienError).code).toBe("COMMAND_RECEIVER_CONFIG_INVALID")
    expect((err as AlienError).context).toMatchObject({ envVar: "ALIEN_COMMANDS_URL" })
  })
})

// ---------------------------------------------------------------------------
// Lease → handle → submit round trips (against the stub)
// ---------------------------------------------------------------------------

describe("CommandReceiver.run", () => {
  it("leases, gives the handler the input bytes/deadline/attempt, and submits the JSON success", async () => {
    // The stub needs the envelope, which needs the base url: bind, then reopen.
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl, attempt: 2 })
    const serve = leaseOnce([lease(env, { attempt: 2 })])
    route = req => serve(req) ?? { status: 200 }

    let seen: { input: string; deadline: number; attempt: number; commandId: string } | undefined
    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", (ctx: CommandContext) => {
      seen = {
        input: new TextDecoder().decode(ctx.input),
        deadline: ctx.deadline.getTime(),
        attempt: ctx.attempt,
        commandId: ctx.commandId,
      }
      return { echoed: JSON.parse(new TextDecoder().decode(ctx.input)) }
    })
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    r.stop()
    await running

    expect(seen?.input).toBe(JSON.stringify({ key: "value" }))
    expect(seen?.attempt).toBe(2)
    expect(seen?.commandId).toBe("cmd_1")
    expect(seen?.deadline).toBeGreaterThan(Date.now())

    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "success" }>
    expect(body.status).toBe("success")
    expect(decodeInline(body)).toEqual({ echoed: { key: "value" } })
  })

  it("submits UNKNOWN_COMMAND when no handler is registered", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl, command: "nobody-home" })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "error" }>
    expect(body.status).toBe("error")
    expect(body.code).toBe("UNKNOWN_COMMAND")
    expect(body.message).toContain("nobody-home")
  })

  it("maps a throwing handler to HANDLER_ERROR", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => {
      throw new Error("database on fire")
    })
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "error" }>
    expect(body.code).toBe("HANDLER_ERROR")
    expect(body.message).toContain("database on fire")
  })

  it("aborts on budget expiry: fires the signal, submits HANDLER_TIMEOUT, drops the late result", async () => {
    server = await openServer()
    const env = envelope({
      baseUrl: server.baseUrl,
      deadline: new Date(Date.now() + 30).toISOString(),
    })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    let signalFired = false
    let completed = false
    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", async (ctx: CommandContext) => {
      ctx.signal.addEventListener("abort", () => {
        signalFired = true
      })
      await new Promise(res => setTimeout(res, 150))
      completed = true
      return { done: true }
    })
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "error" }>
    expect(body.code).toBe("HANDLER_TIMEOUT")
    expect(signalFired).toBe(true)
    expect(completed).toBe(false)

    // Let the late handler resolve, then confirm no second submit happened.
    await new Promise(res => setTimeout(res, 200))
    const submits = server.requests.filter(
      req => req.method === "PUT" && req.path === "/v1/commands/cmd_1/response",
    )
    expect(submits).toHaveLength(1)
  })

  it("passes the lease attempt through to the handler", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl, attempt: 4 })
    const serve = leaseOnce([lease(env, { attempt: 4 })])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", (ctx: CommandContext) => ({ attempt: ctx.attempt }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "success" }>
    expect(decodeInline(body)).toEqual({ attempt: 4 })
  })

  it("decodes storage-mode (http backend) command input", async () => {
    server = await openServer()
    const params = { fromStorage: true, n: 9 }
    const env = envelope({
      baseUrl: server.baseUrl,
      params: {
        mode: "storage",
        size: 20,
        storageGetRequest: {
          backend: { type: "http", url: `${server.baseUrl}/blob`, method: "GET", headers: {} },
          expiration: new Date(Date.now() + 60_000).toISOString(),
          operation: "get",
          path: "blob",
        },
      },
    })
    const serve = leaseOnce([lease(env)])
    route = req => {
      if (req.method === "GET" && req.path === "/blob") return { text: JSON.stringify(params) }
      return serve(req) ?? { status: 200 }
    }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", (ctx: CommandContext) => JSON.parse(new TextDecoder().decode(ctx.input)))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "success" }>
    expect(decodeInline(body)).toEqual(params)
  })

  it("overflows a large response to a presigned storage PUT", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    env.responseHandling.maxInlineBytes = 5 // force overflow
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const big = { payload: "x".repeat(64) }
    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => big)
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)

    const upload = server.requests.find(req => req.method === "PUT" && req.path === "/storage-put")
    expect(upload).toBeDefined()

    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "success" }>
    expect(body.response.mode).toBe("storage")
    if (body.response.mode === "storage") {
      expect(body.response.storagePutUsed).toBe(true)
      expect(body.response.size).toBe(JSON.stringify(big).length)
    }
  })

  it("drains: stop() lets the in-flight command finish and stops further lease polls", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    let release!: () => void
    const gate = new Promise<void>(res => {
      release = res
    })

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    let handlerStarted = false
    r.handle("echo", async () => {
      handlerStarted = true
      await gate
      return { drained: true }
    })
    receiverStop = () => {
      release()
      r.stop()
    }
    running = r.run()

    await waitFor(() => handlerStarted)
    r.stop() // request shutdown while the handler is still in flight
    release() // let the in-flight handler complete
    await running // run() only resolves after the in-flight command drains

    expect(submitBody("cmd_1")).toBeDefined()

    // No further lease polls after run() returned.
    const leasePolls = () =>
      server?.requests.filter(req => req.method === "POST" && req.path === "/v1/commands/leases")
        .length ?? 0
    const after = leasePolls()
    await new Promise(res => setTimeout(res, 40)) // >> pollIntervalMs
    expect(leasePolls()).toBe(after)
  })

  it("submits INVALID_ENVELOPE for malformed inline base64 params (twin of Rust's decode_params_bytes)", async () => {
    server = await openServer()
    const env = envelope({
      baseUrl: server.baseUrl,
      params: { mode: "inline", inlineBase64: "not-valid-base64!!" },
    })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => ({ ok: true }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "error" }>
    expect(body.status).toBe("error")
    expect(body.code).toBe("INVALID_ENVELOPE")
  })

  it("submits INVALID_ENVELOPE when storage params are missing storageGetRequest (twin-pinned)", async () => {
    server = await openServer()
    const env = envelope({
      baseUrl: server.baseUrl,
      params: { mode: "storage" },
    })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => ({ ok: true }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    const body = submitBody("cmd_1") as Extract<CommandResponse, { status: "error" }>
    expect(body.status).toBe("error")
    expect(body.code).toBe("INVALID_ENVELOPE")
  })

  it("sends the lease POST with typed target, defaults, and bearer auth (mirrors Rust's lease_request_carries_typed_target_and_defaults)", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => ({ ok: true }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)

    const leaseReq = server.requests.find(
      req => req.method === "POST" && req.path === "/v1/commands/leases",
    )
    expect(leaseReq).toBeDefined()
    expect(leaseReq?.body).toEqual({
      deploymentId: "dep-123",
      target: { resourceId: "agent", resourceType: "daemon" },
      maxLeases: 10,
      leaseSeconds: 60,
    })
    expect(leaseReq?.headers.authorization).toBe("Bearer tok")
  })

  it("builds the lease endpoint from a query-string base URL without corrupting path/query (M1 — mirrors Rust's path_segments_mut)", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })

    let leaseServed = false
    let capturedLeasePath: string | undefined
    route = req => {
      if (req.method === "POST" && req.path.startsWith("/v1/commands/leases")) {
        capturedLeasePath = req.path
        if (leaseServed) return { json: { leases: [] } }
        leaseServed = true
        return { json: { leases: [lease(env)] } }
      }
      if (req.method === "PUT") return { status: 200 }
      return { status: 404 }
    }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1?token=abc` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => ({ ok: true }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(() => submitBody("cmd_1") !== undefined)
    expect(capturedLeasePath).toBe("/v1/commands/leases?token=abc")
  })

  it("rejects an expired presigned upload before attempting the PUT (M2)", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    env.responseHandling.maxInlineBytes = 5 // force overflow to storage
    env.responseHandling.storageUploadRequest.expiration = new Date(
      Date.now() - 60_000,
    ).toISOString()
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const big = { payload: "x".repeat(64) }
    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => big)
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(
      () =>
        (server?.requests.filter(req => req.method === "POST" && req.path === "/v1/commands/leases")
          .length ?? 0) >= 1,
    )
    // No ack path exists for this failure — give the (rejected) submit attempt
    // time to run, then assert it never reached the storage PUT.
    await new Promise(res => setTimeout(res, 60))

    const upload = server.requests.find(req => req.method === "PUT" && req.path === "/storage-put")
    expect(upload).toBeUndefined()
    expect(submitBody("cmd_1")).toBeUndefined()
  })

  it("rejects a path-traversal local upload backend before writing (M2)", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    env.responseHandling.maxInlineBytes = 5 // force overflow to storage
    env.responseHandling.storageUploadRequest = {
      backend: { type: "local", filePath: "../evil.json", operation: "put" },
      expiration: new Date(Date.now() + 3_600_000).toISOString(),
      operation: "put",
      path: "resp-path",
    }
    const serve = leaseOnce([lease(env)])
    route = req => serve(req) ?? { status: 200 }

    const big = { payload: "x".repeat(64) }
    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => big)
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(
      () =>
        (server?.requests.filter(req => req.method === "POST" && req.path === "/v1/commands/leases")
          .length ?? 0) >= 1,
    )
    await new Promise(res => setTimeout(res, 60))

    expect(submitBody("cmd_1")).toBeUndefined()
  })

  it("does not submit twice when the submit fails (no ack → redelivery)", async () => {
    server = await openServer()
    const env = envelope({ baseUrl: server.baseUrl })
    const serve = leaseOnce([lease(env)])
    route = req => {
      if (req.method === "PUT" && req.path === "/v1/commands/cmd_1/response") {
        return { status: 500, text: "nope" }
      }
      return serve(req) ?? { status: 200 }
    }

    const r = createCommandReceiver({
      env: { ...FULL_ENV, ALIEN_COMMANDS_URL: `${server.baseUrl}/v1/` },
      pollIntervalMs: 5,
    })
    r.handle("echo", () => ({ ok: true }))
    receiverStop = () => r.stop()
    running = r.run()

    await waitFor(
      () =>
        (server?.requests.filter(
          req => req.method === "PUT" && req.path === "/v1/commands/cmd_1/response",
        ).length ?? 0) >= 1,
    )
    // Give any (incorrect) retry a chance to happen.
    await new Promise(res => setTimeout(res, 40))
    const submits = server.requests.filter(
      req => req.method === "PUT" && req.path === "/v1/commands/cmd_1/response",
    )
    expect(submits).toHaveLength(1)
  })
})
