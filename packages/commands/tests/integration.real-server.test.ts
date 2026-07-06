/**
 * Real-wire integration suite: the TS sender and receiver twins driven against
 * the ACTUAL Rust command server (`crates/alien-commands` `test-command-server`
 * bin, `test-utils` feature). No stub, no mock — every byte crosses a real HTTP
 * socket to the same axum router production uses.
 *
 * The bin is compiled once in `beforeAll` (`cargo build … --message-format=json`,
 * which also tells us the executable path), then each test spawns that prebuilt
 * binary directly and parses its `READY {base_url}` line.
 *
 * This suite FAILS LOUDLY if cargo or the server is unavailable — there is no
 * silent skip. It runs in CI (where cargo is present and warm) and locally.
 *
 * Runs under Node (vitest) only; it is excluded from the default `vitest run`
 * and the Bun canary, and wired as the dedicated `test:integration` script.
 */

import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { AlienError } from "@alienplatform/core"
import { afterEach, beforeAll, describe, expect, it } from "vitest"
import { CommandsClient } from "../src/client.js"
import type { LeaseInfo, LeaseResponse } from "../src/protocol.js"
import { type CommandContext, createCommandReceiver } from "../src/receiver.js"

// Workspace root (…/tests → …/commands → …/packages → root) — the cwd cargo runs in.
const WORKSPACE_ROOT = fileURLToPath(new URL("../../../", import.meta.url))
const CARGO = process.env.CARGO ?? "cargo"

const DEPLOYMENT_ID = "test-deployment"
const TARGET_RESOURCE_ID = "test-daemon"
const TARGET_RESOURCE_TYPE = "daemon"
const TOKEN = "test-token"

/** Path to the prebuilt bin, resolved once in beforeAll. */
let serverBinPath = ""

/**
 * Run the cargo build asynchronously and collect its JSON artifact stream.
 *
 * Deliberately NOT `execFileSync`: a cold CI build (compile + link) can take
 * minutes, and a synchronous spawn blocks this worker's event loop for that
 * whole duration — vitest's worker↔main RPC misses its heartbeats and reports
 * a spurious `[vitest-worker]: Timeout calling "onTaskUpdate"` unhandled error
 * even though every test passes. No `*Sync` child-process calls anywhere in
 * this suite; the event loop stays responsive and only the generous
 * `hookTimeout` bounds the build.
 */
function runCargoBuild(): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const proc = spawn(
      CARGO,
      [
        "build",
        "-p",
        "alien-commands",
        "--features",
        "test-utils",
        "--bin",
        "test-command-server",
        "--message-format=json",
      ],
      { cwd: WORKSPACE_ROOT, stdio: ["ignore", "pipe", "pipe"] },
    )

    let stdout = ""
    let stderr = ""
    proc.stdout.setEncoding("utf-8")
    proc.stdout.on("data", (chunk: string) => {
      stdout += chunk
    })
    proc.stderr.setEncoding("utf-8")
    proc.stderr.on("data", (chunk: string) => {
      stderr += chunk
    })
    proc.on("error", err => {
      reject(new Error(`failed to spawn cargo: ${err.message}`))
    })
    proc.on("close", code => {
      if (code === 0) {
        resolve(stdout)
      } else {
        reject(new Error(`cargo build failed with exit code ${code}. stderr:\n${stderr}`))
      }
    })
  })
}

/**
 * Build the test-command-server bin once and resolve its executable path from
 * cargo's JSON artifact stream. Fails loudly (throws) if cargo is missing or the
 * build fails — the whole suite is meaningless without the real server.
 */
beforeAll(async () => {
  const stdout = await runCargoBuild()

  for (const line of stdout.split("\n")) {
    if (!line.trim()) continue
    let msg: { reason?: string; executable?: string | null; target?: { name?: string } }
    try {
      msg = JSON.parse(line)
    } catch {
      continue
    }
    if (
      msg.reason === "compiler-artifact" &&
      msg.target?.name === "test-command-server" &&
      msg.executable
    ) {
      serverBinPath = msg.executable
    }
  }

  // Fallback: default target dir layout, if the artifact stream didn't name it.
  if (!serverBinPath) {
    const fallback = fileURLToPath(
      new URL("target/debug/test-command-server", `file://${WORKSPACE_ROOT}`),
    )
    if (existsSync(fallback)) serverBinPath = fallback
  }

  if (!serverBinPath || !existsSync(serverBinPath)) {
    throw new Error(
      `test-command-server binary not found after cargo build (looked at '${serverBinPath}'). The real-wire integration suite cannot run without it.`,
    )
  }
}, 600_000)

interface RealServer {
  /** Sender base URL (router is nested under /v1, which the sender appends). */
  managerUrl: string
  /** Receiver `ALIEN_COMMANDS_URL` (the /v1 command base). */
  commandsUrl: string
  stop: () => Promise<void>
}

/** Spawn the prebuilt bin and wait for its `READY {base_url}` line. */
async function startRealServer(): Promise<RealServer> {
  const proc: ChildProcessWithoutNullStreams = spawn(serverBinPath, [], {
    cwd: WORKSPACE_ROOT,
    stdio: ["pipe", "pipe", "pipe"],
  }) as ChildProcessWithoutNullStreams

  let stderr = ""
  proc.stderr.setEncoding("utf-8")
  proc.stderr.on("data", chunk => {
    stderr += chunk
  })

  const baseUrl = await new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`test-command-server did not print READY within 15s. stderr:\n${stderr}`))
    }, 15_000)

    let buffer = ""
    proc.stdout.setEncoding("utf-8")
    proc.stdout.on("data", (chunk: string) => {
      buffer += chunk
      const match = buffer.match(/^READY (\S+)/m)
      if (match?.[1]) {
        clearTimeout(timer)
        resolve(match[1])
      }
    })
    proc.on("exit", code => {
      clearTimeout(timer)
      reject(new Error(`test-command-server exited early (code ${code}). stderr:\n${stderr}`))
    })
    proc.on("error", err => {
      clearTimeout(timer)
      reject(new Error(`failed to spawn test-command-server: ${err.message}`))
    })
  })

  const stop = () =>
    new Promise<void>(resolve => {
      const finish = () => {
        // Detach the pipes explicitly rather than relying on process exit to
        // reclaim them — an undestroyed stream can keep the event loop (and
        // vitest's worker↔main RPC) busy past teardown.
        proc.stdout.destroy()
        proc.stderr.destroy()
        resolve()
      }

      if (proc.exitCode !== null || proc.signalCode !== null) {
        finish()
        return
      }

      proc.once("exit", () => {
        clearTimeout(killTimer)
        finish()
      })

      // Closing stdin is the bin's graceful shutdown; SIGTERM backs it up in
      // case the bin doesn't treat stdin-close as a shutdown signal, and
      // SIGKILL is the hard fallback if it ignores both.
      proc.stdin.end()
      proc.kill("SIGTERM")
      const killTimer = setTimeout(() => proc.kill("SIGKILL"), 2_000)
    })

  return {
    managerUrl: baseUrl,
    commandsUrl: `${baseUrl}/v1`,
    stop,
  }
}

function makeSender(server: RealServer): CommandsClient {
  return new CommandsClient({
    managerUrl: server.managerUrl,
    deploymentId: DEPLOYMENT_ID,
    token: TOKEN,
  })
}

function receiverEnv(server: RealServer): Record<string, string> {
  return {
    ALIEN_COMMANDS_URL: server.commandsUrl,
    ALIEN_COMMANDS_TOKEN: TOKEN,
    ALIEN_DEPLOYMENT_ID: DEPLOYMENT_ID,
    ALIEN_COMMANDS_TARGET_RESOURCE_ID: TARGET_RESOURCE_ID,
    ALIEN_COMMANDS_TARGET_RESOURCE_TYPE: TARGET_RESOURCE_TYPE,
  }
}

/** Fast sender polling so round-trips resolve in tens of ms, not seconds. */
const FAST_POLL = { pollIntervalMs: 40, maxPollIntervalMs: 200, timeoutMs: 20_000 }

/** Harness lease request against the daemon target (bypasses the receiver). */
async function harnessLease(server: RealServer): Promise<LeaseInfo | undefined> {
  const res = await fetch(`${server.commandsUrl}/commands/leases`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${TOKEN}` },
    body: JSON.stringify({
      deploymentId: DEPLOYMENT_ID,
      target: { resourceId: TARGET_RESOURCE_ID, resourceType: TARGET_RESOURCE_TYPE },
      maxLeases: 1,
      leaseSeconds: 60,
    }),
  })
  if (!res.ok) throw new Error(`harness lease failed: ${res.status} ${await res.text()}`)
  const body = (await res.json()) as LeaseResponse
  return body.leases?.[0]
}

/** Harness lease release — increments the server's attempt counter (redelivery). */
async function harnessRelease(server: RealServer, leaseId: string): Promise<void> {
  const res = await fetch(`${server.commandsUrl}/commands/leases/${leaseId}/release`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}` },
  })
  if (!res.ok) throw new Error(`harness release failed: ${res.status} ${await res.text()}`)
}

const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))

describe("real-wire command twins", () => {
  let server: RealServer | undefined

  afterEach(async () => {
    await server?.stop()
    server = undefined
  })

  it("full twin loop: sender invoke → real receiver leases/handles/submits → sender resolves", async () => {
    server = await startRealServer()

    let seen: CommandContext | undefined
    const receiver = createCommandReceiver({
      env: receiverEnv(server),
      pollIntervalMs: 25,
      leaseSeconds: 60,
    })
    receiver.handle("echo", ctx => {
      seen = ctx
      const params = JSON.parse(new TextDecoder().decode(ctx.input)) as Record<string, unknown>
      return { echoed: params, attempt: ctx.attempt }
    })
    const running = receiver.run()

    const result = await makeSender(server)
      .target(TARGET_RESOURCE_ID)
      .invoke<{ echoed: { hello: string }; attempt: number }>("echo", { hello: "world" }, FAST_POLL)

    receiver.stop()
    await running

    // Sender resolved with the handler's return, round-tripped over the real wire.
    expect(result).toEqual({ echoed: { hello: "world" }, attempt: 1 })
    // The handler observed the twin-identity context: decoded input bytes,
    // attempt 1, and a Date deadline.
    expect(seen).toBeDefined()
    expect(JSON.parse(new TextDecoder().decode(seen!.input))).toEqual({ hello: "world" })
    expect(seen!.attempt).toBe(1)
    expect(seen!.deadline).toBeInstanceOf(Date)
  }, 30_000)

  it("budget: short lease → slow handler → real lease-expiry fires signal → HANDLER_TIMEOUT surfaced", async () => {
    server = await startRealServer()

    let signalFired = false
    const receiver = createCommandReceiver({
      env: receiverEnv(server),
      // Poll fast so the command is caught right after creation; the short
      // lease then bounds the budget. Once HANDLER_TIMEOUT settles the command
      // Failed (terminal), re-leasing is a no-op, so a small interval is safe.
      pollIntervalMs: 25,
      leaseSeconds: 1,
    })
    receiver.handle("slow", async ctx => {
      // Deliberately ignore the abort signal (only record it) and outlive the
      // budget so the receiver must time it out on us.
      ctx.signal.addEventListener("abort", () => {
        signalFired = true
      })
      await sleep(10_000)
      return { neverReached: true }
    })
    const running = receiver.run()

    const err = await makeSender(server)
      .target(TARGET_RESOURCE_ID)
      .invoke("slow", { work: "forever" }, FAST_POLL)
      .catch((e: unknown) => e)

    receiver.stop()
    await running

    // The real lease's expiry drove the budget: the signal fired in-process…
    expect(signalFired).toBe(true)
    // …and the receiver submitted HANDLER_TIMEOUT, which the sender surfaces.
    expect(err).toBeInstanceOf(AlienError)
    const alien = err as AlienError
    expect(alien.code).toBe("DEPLOYMENT_COMMAND_ERROR")
    expect(alien.context).toMatchObject({ errorCode: "HANDLER_TIMEOUT" })
  }, 30_000)

  it("redelivery: released lease bumps the server attempt → receiver observes attempt 2", async () => {
    server = await startRealServer()
    const sender = makeSender(server)

    // Kick off the invoke in the background: it creates the command and polls.
    const invoked = sender
      .target(TARGET_RESOURCE_ID)
      .invoke<{ ok: boolean; attempt: number }>("redeliver", { n: 1 }, FAST_POLL)

    // Simulate a crashed/expired first delivery: lease it ourselves (attempt 1),
    // then release it — which increments the server's attempt counter and
    // returns the command to Pending (the real-wire twin of the reaper path).
    let firstLease: LeaseInfo | undefined
    for (let i = 0; i < 100 && !firstLease; i++) {
      firstLease = await harnessLease(server)
      if (!firstLease) await sleep(20)
    }
    expect(firstLease).toBeDefined()
    expect(firstLease!.attempt).toBe(1)
    await harnessRelease(server, firstLease!.leaseId)

    // Now bring up the real receiver; it must lease the redelivered command
    // and see the incremented attempt.
    const receiver = createCommandReceiver({
      env: receiverEnv(server),
      pollIntervalMs: 25,
      leaseSeconds: 60,
    })
    receiver.handle("redeliver", ctx => ({ ok: true, attempt: ctx.attempt }))
    const running = receiver.run()

    const result = await invoked
    receiver.stop()
    await running

    expect(result).toEqual({ ok: true, attempt: 2 })
  }, 30_000)

  it("UNKNOWN_COMMAND: no handler → command settles Failed → sender surfaces DeploymentCommandError", async () => {
    server = await startRealServer()

    // Receiver with a handler for a DIFFERENT command, so the leased command
    // has no handler and the receiver submits UNKNOWN_COMMAND.
    const receiver = createCommandReceiver({
      env: receiverEnv(server),
      pollIntervalMs: 25,
      leaseSeconds: 60,
    })
    receiver.handle("something-else", () => ({}))
    const running = receiver.run()

    const err = await makeSender(server)
      .target(TARGET_RESOURCE_ID)
      .invoke("mystery", {}, FAST_POLL)
      .catch((e: unknown) => e)

    receiver.stop()
    await running

    expect(err).toBeInstanceOf(AlienError)
    const alien = err as AlienError
    expect(alien.code).toBe("DEPLOYMENT_COMMAND_ERROR")
    expect(alien.context).toMatchObject({ errorCode: "UNKNOWN_COMMAND" })
  }, 30_000)
})
