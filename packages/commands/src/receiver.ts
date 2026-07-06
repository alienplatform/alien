/**
 * Pull command receiver — the app-owned lease loop for Containers and Daemons.
 *
 * Behavior-identical twin of the Rust `alien_commands::Receiver`. It leases
 * commands addressed to its own target resource from the command server over
 * outbound HTTPS (no inbound connections, no gRPC), dispatches them to
 * in-process handlers, and submits responses through the envelope's
 * response-handling flow (inline or presigned storage upload).
 *
 * Pure `fetch`; no bindings, no gRPC. See `PACKAGE_LAYOUT.md` DECIDED(09) for
 * the binding semantics this file implements:
 * - env quintet, fail-fast naming the offending variable (worker type rejected)
 * - execution budget = `min(envelope.deadline, leaseExpiresAt)`; on expiry the
 *   handler's `signal` fires and a `HANDLER_TIMEOUT` response is submitted
 * - error codes `UNKNOWN_COMMAND` / `HANDLER_ERROR` / `HANDLER_TIMEOUT`, with a
 *   params-decode failure submitted under the decode error's own code
 * - at-least-once delivery; `ctx.attempt` starts at 1
 * - drain: shutdown stops *starting* new polls; in-flight commands complete
 * - lease params 5s / maxLeases 10 / leaseSeconds 60
 * - `ctx.input` = decoded param bytes; success body = `JSON.stringify(return)`;
 *   `ctx.deadline` = the effective budget, always present while a lease is held
 *
 * # Bootstrap
 *
 * ```typescript
 * import { createCommandReceiver } from "@alienplatform/commands"
 *
 * const receiver = createCommandReceiver()
 * receiver.handle("generate-report", async ctx => {
 *   const params = JSON.parse(new TextDecoder().decode(ctx.input))
 *   return { report: params }
 * })
 * await receiver.run() // call receiver.stop() to drain and return
 * ```
 */

import { AlienError } from "@alienplatform/core"
import {
  CommandReceiverConfigInvalidError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"
import type {
  BodySpec,
  CommandResponse,
  CommandTargetType,
  Envelope,
  LeaseInfo,
  LeaseRequest,
  LeaseResponse,
  PresignedRequest,
} from "./protocol.js"

/** Error code submitted when a leased command has no registered handler. */
export const ERROR_CODE_UNKNOWN_COMMAND = "UNKNOWN_COMMAND"
/** Error code submitted when a handler exceeds its execution budget. */
export const ERROR_CODE_HANDLER_TIMEOUT = "HANDLER_TIMEOUT"
/** Error code submitted when a handler throws/rejects (or its response fails to serialize). */
export const ERROR_CODE_HANDLER_ERROR = "HANDLER_ERROR"

/** Lease poll interval, in ms (DECIDED(09) — 5s). */
const DEFAULT_POLL_INTERVAL_MS = 5_000
/** Max leases requested per poll (DECIDED(09) — 10). */
const DEFAULT_MAX_LEASES = 10
/** Requested lease duration, in seconds (DECIDED(09) — 60). */
const DEFAULT_LEASE_SECONDS = 60

// Env variable names — identical strings to the Rust twin
// (`alien_core::runtime_environment`).
const ENV_ALIEN_COMMANDS_URL = "ALIEN_COMMANDS_URL"
const ENV_ALIEN_COMMANDS_TOKEN = "ALIEN_COMMANDS_TOKEN"
const ENV_ALIEN_DEPLOYMENT_ID = "ALIEN_DEPLOYMENT_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID = "ALIEN_COMMANDS_TARGET_RESOURCE_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE = "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE"

/**
 * Per-command context passed to a {@link CommandHandler}.
 *
 * DECIDED(08) — the concrete handler-context field types. These are the twin of
 * the Rust `Context` struct (`input`/`deadline`/`command_id`/`attempt`/
 * `cancellation`), the last mapping to `signal` here.
 */
export interface CommandContext {
  /**
   * Decoded command param bytes — the same bytes the params envelope carries
   * after decode, prior to any handler-side parsing (DECIDED(09), byte-for-byte
   * twin identity with the Rust receiver's `ctx.input`).
   */
  input: Uint8Array
  /**
   * Fires when the execution budget expires. The handler promise is abandoned
   * regardless; observe this to stop cooperative work the handler started.
   * Twin of the Rust `ctx.cancellation` token.
   */
  signal: AbortSignal
  /**
   * The effective execution budget: `min(envelope.deadline, leaseExpiresAt)`.
   * Always present while a lease is held (DECIDED(09)).
   */
  deadline: Date
  /** Unique command identifier. */
  commandId: string
  /**
   * Delivery attempt, starting at 1. Greater than 1 means redelivery
   * (at-least-once semantics); handlers must tolerate running more than once.
   */
  attempt: number
}

/**
 * A command handler. Receives a {@link CommandContext} and returns any
 * JSON-serializable value, submitted as the command's success response
 * (`JSON.stringify`-encoded). Throwing/rejecting submits a `HANDLER_ERROR`.
 */
export type CommandHandler = (ctx: CommandContext) => unknown | Promise<unknown>

/**
 * The pull receiver handle. Register handlers with {@link CommandReceiver.handle},
 * drive the lease loop with {@link CommandReceiver.run}, and stop it (draining
 * in-flight commands) with {@link CommandReceiver.stop}.
 */
export interface CommandReceiver {
  /** Register a handler for a command name. Registering a name twice replaces it. */
  handle(name: string, handler: CommandHandler): CommandReceiver
  /**
   * Drive the lease loop until {@link CommandReceiver.stop} is called. No new
   * lease poll *starts* once draining begins; a poll already in flight
   * completes and its leases are dispatched and drained. Every in-flight
   * command finishes within its budget before this resolves.
   */
  run(): Promise<void>
  /** Signal the receiver to drain and stop (see {@link CommandReceiver.run}). */
  stop(): void
}

/** Options for {@link createCommandReceiver}; every field has a production default. */
export interface CommandReceiverOptions {
  /** Environment source (defaults to `process.env`). */
  env?: Record<string, string | undefined>
  /** `fetch` implementation (defaults to the global `fetch`). */
  fetch?: typeof fetch
  /** Lease poll interval in ms (default 5000). Mainly for tests. */
  pollIntervalMs?: number
  /** Requested lease duration in seconds (default 60). Mainly for tests. */
  leaseSeconds?: number
  /** Max leases requested per poll (default 10). Mainly for tests. */
  maxLeases?: number
}

interface ReceiverConfig {
  url: string
  token: string
  deploymentId: string
  resourceId: string
  resourceType: CommandTargetType
}

/**
 * Construct the pull receiver from environment configuration.
 *
 * Validates the required env quintet **synchronously** — an empty, missing, or
 * invalid value throws {@link CommandReceiverConfigInvalidError} naming the
 * offending variable in `context.envVar` (DECIDED(09)). `resourceType` must be
 * `container` or `daemon`; `worker` (and anything else) is rejected — a receiver
 * must not guess its target type.
 */
export function createCommandReceiver(options: CommandReceiverOptions = {}): CommandReceiver {
  const env = options.env ?? (typeof process !== "undefined" ? process.env : {})
  const config = validateConfig(env)
  return new PullCommandReceiver(config, options)
}

function requireEnv(env: Record<string, string | undefined>, name: string): string {
  const value = env[name]
  if (value === undefined) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({ envVar: name, reason: `${name} is required` }),
    )
  }
  if (value === "") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: name,
        reason: `${name} must not be empty`,
      }),
    )
  }
  return value
}

function validateConfig(env: Record<string, string | undefined>): ReceiverConfig {
  const url = requireEnv(env, ENV_ALIEN_COMMANDS_URL)
  try {
    // eslint-disable-next-line no-new -- validating parseability only
    new URL(url)
  } catch {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_URL,
        reason: `${ENV_ALIEN_COMMANDS_URL} is not a valid URL: ${url}`,
      }),
    )
  }

  const token = requireEnv(env, ENV_ALIEN_COMMANDS_TOKEN)
  const deploymentId = requireEnv(env, ENV_ALIEN_DEPLOYMENT_ID)
  const resourceId = requireEnv(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID)

  const rawType = requireEnv(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE)
  if (rawType !== "container" && rawType !== "daemon") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
        reason: `${ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE} must be 'container' or 'daemon', got '${rawType}'`,
      }),
    )
  }

  return { url, token, deploymentId, resourceId, resourceType: rawType }
}

class PullCommandReceiver implements CommandReceiver {
  private readonly config: ReceiverConfig
  private readonly fetchImpl: typeof fetch
  private readonly pollIntervalMs: number
  private readonly leaseSeconds: number
  private readonly maxLeases: number
  private readonly handlers = new Map<string, CommandHandler>()
  private readonly shutdown = new AbortController()

  constructor(config: ReceiverConfig, options: CommandReceiverOptions) {
    this.config = config
    this.fetchImpl = options.fetch ?? globalThis.fetch
    this.pollIntervalMs = options.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS
    this.leaseSeconds = options.leaseSeconds ?? DEFAULT_LEASE_SECONDS
    this.maxLeases = options.maxLeases ?? DEFAULT_MAX_LEASES
  }

  handle(name: string, handler: CommandHandler): CommandReceiver {
    this.handlers.set(name, handler)
    return this
  }

  stop(): void {
    this.shutdown.abort()
  }

  async run(): Promise<void> {
    const inFlight = new Set<Promise<void>>()

    // Mirrors the Rust run loop: check shutdown at the top of each iteration
    // (no new poll starts once draining begins), acquire leases (a poll already
    // in flight completes and its leases are dispatched), then sleep-or-stop.
    while (!this.shutdown.signal.aborted) {
      let leases: LeaseInfo[] = []
      try {
        leases = await this.acquireLeases()
      } catch (error) {
        // Transient lease errors are logged and retried next interval.
        logWarn("Failed to acquire command leases, will retry", error)
      }

      for (const lease of leases) {
        const task = this.processLease(lease)
        inFlight.add(task)
        void task.finally(() => inFlight.delete(task))
      }

      if (this.shutdown.signal.aborted) {
        break
      }
      await this.sleepOrStop(this.pollIntervalMs)
    }

    // Drain: every in-flight command finishes within its own budget.
    await Promise.all([...inFlight])
  }

  /** Build the lease request this receiver sends (pure — unit-testable). */
  private buildLeaseRequest(): LeaseRequest {
    return {
      deploymentId: this.config.deploymentId,
      target: {
        resourceId: this.config.resourceId,
        resourceType: this.config.resourceType,
      },
      maxLeases: this.maxLeases,
      leaseSeconds: this.leaseSeconds,
    }
  }

  private async acquireLeases(): Promise<LeaseInfo[]> {
    const endpoint = `${this.config.url.replace(/\/+$/, "")}/commands/leases`
    const response = await this.fetchImpl(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.config.token}`,
      },
      body: JSON.stringify(this.buildLeaseRequest()),
    })

    if (!response.ok) {
      const body = await response.text().catch(() => "")
      throw new Error(`Lease request failed with status ${response.status}: ${body}`)
    }

    const parsed = (await response.json()) as LeaseResponse
    return parsed.leases ?? []
  }

  /**
   * Process one leased command end to end: execute (or reject) it, then submit
   * exactly one response. Only this path submits — a handler cannot double
   * submit, and a submit failure produces no ack, so the lease expires and the
   * command is redelivered.
   */
  private async processLease(lease: LeaseInfo): Promise<void> {
    const response = await this.executeLease(lease)
    try {
      await this.submitResponse(lease.envelope, response)
    } catch (error) {
      // No ack: the lease will expire and the command is redelivered.
      logError(`Failed to submit response for command '${lease.commandId}'`, error)
    }
  }

  /**
   * Execute a leased command under its budget and produce the response to
   * submit. Never submits (keeps the "only the loop submits, once" invariant).
   */
  private async executeLease(lease: LeaseInfo): Promise<CommandResponse> {
    const { envelope, leaseExpiresAt, attempt } = lease
    const handler = this.handlers.get(envelope.command)
    if (!handler) {
      return errorResponse(
        ERROR_CODE_UNKNOWN_COMMAND,
        `No handler registered for command '${envelope.command}'`,
      )
    }

    let input: Uint8Array
    try {
      input = await decodeParamsBytes(envelope, this.fetchImpl)
    } catch (error) {
      // Decode failure is submitted under the decode error's own code, not a
      // receiver-specific one (DECIDED(09)).
      const code = error instanceof AlienError ? error.code : ERROR_CODE_HANDLER_ERROR
      return errorResponse(code, error instanceof Error ? error.message : String(error))
    }

    const budget = commandBudget(envelope.deadline, leaseExpiresAt)
    const controller = new AbortController()
    const ctx: CommandContext = {
      input,
      signal: controller.signal,
      deadline: budget,
      commandId: envelope.commandId,
      attempt,
    }

    return runUnderBudget(handler, ctx, budget, controller, envelope.command)
  }

  private async submitResponse(envelope: Envelope, response: CommandResponse): Promise<void> {
    let finalResponse = response

    if (response.status === "success" && response.response.mode === "inline") {
      const bytes = base64ToBytes(response.response.inlineBase64)
      const maxInline = envelope.responseHandling.maxInlineBytes
      if (bytes.byteLength > maxInline) {
        // Large response: upload to storage first, then reference it.
        await this.uploadResponseToStorage(envelope.responseHandling.storageUploadRequest, bytes)
        finalResponse = {
          status: "success",
          response: { mode: "storage", size: bytes.byteLength, storagePutUsed: true },
        }
      }
    }

    // The submit URL is fully qualified and pre-authorized by the envelope, so
    // it carries no bearer header (matching the Rust twin's submit path).
    const res = await this.fetchImpl(envelope.responseHandling.submitResponseUrl, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(finalResponse),
    })

    if (!res.ok) {
      const body = await res.text().catch(() => "")
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: envelope.responseHandling.submitResponseUrl,
          reason: `Response submission failed with status ${res.status}: ${body}`,
        }),
      )
    }
  }

  private async uploadResponseToStorage(
    request: PresignedRequest,
    bytes: Uint8Array,
  ): Promise<void> {
    if (request.backend.type === "http") {
      const res = await this.fetchImpl(request.backend.url, {
        method: request.backend.method,
        headers: request.backend.headers,
        body: bytes,
      })
      if (!res.ok) {
        throw new AlienError(
          StorageOperationFailedError.create({
            operation: "upload",
            url: request.backend.url,
            reason: `Storage upload failed with status ${res.status}`,
          }),
        )
      }
      return
    }

    // Local backend (dev only).
    const { writeFile } = await import("node:fs/promises")
    await writeFile(request.backend.filePath, bytes)
  }

  private sleepOrStop(ms: number): Promise<void> {
    return new Promise<void>(resolve => {
      if (this.shutdown.signal.aborted) {
        resolve()
        return
      }
      const timer = setTimeout(() => {
        this.shutdown.signal.removeEventListener("abort", onAbort)
        resolve()
      }, ms)
      const onAbort = () => {
        clearTimeout(timer)
        resolve()
      }
      this.shutdown.signal.addEventListener("abort", onAbort, { once: true })
    })
  }
}

/**
 * Per-command execution budget: `min(envelope.deadline, leaseExpiresAt)`. There
 * is no lease-renew call, so the lease expiry always bounds it.
 */
export function commandBudget(deadline: string | undefined, leaseExpiresAt: string): Date {
  const lease = new Date(leaseExpiresAt)
  if (deadline === undefined) {
    return lease
  }
  const envelopeDeadline = new Date(deadline)
  return envelopeDeadline.getTime() < lease.getTime() ? envelopeDeadline : lease
}

/**
 * Run the handler racing a budget timer. On budget expiry the `signal` fires,
 * the handler promise is abandoned (its later settlement is ignored — only this
 * function's return is ever submitted), and a `HANDLER_TIMEOUT` is returned.
 */
async function runUnderBudget(
  handler: CommandHandler,
  ctx: CommandContext,
  budget: Date,
  controller: AbortController,
  command: string,
): Promise<CommandResponse> {
  const remainingMs = Math.max(0, budget.getTime() - Date.now())

  let timer: ReturnType<typeof setTimeout> | undefined
  const budgetPromise = new Promise<{ kind: "timeout" }>(resolve => {
    timer = setTimeout(() => resolve({ kind: "timeout" }), remainingMs)
  })

  const handlerPromise = Promise.resolve()
    .then(() => handler(ctx))
    .then(
      value => ({ kind: "return" as const, value }),
      (error: unknown) => ({ kind: "throw" as const, error }),
    )

  const outcome = await Promise.race([handlerPromise, budgetPromise])
  if (timer !== undefined) {
    clearTimeout(timer)
  }

  if (outcome.kind === "timeout") {
    // Budget expired: fire the signal for cooperative work; abandon the handler
    // promise so a late settlement can't double-submit.
    controller.abort()
    void handlerPromise.catch(() => {})
    return errorResponse(
      ERROR_CODE_HANDLER_TIMEOUT,
      `Command '${command}' exceeded its execution budget (${budget.toISOString()})`,
    )
  }

  if (outcome.kind === "throw") {
    const message = outcome.error instanceof Error ? outcome.error.message : String(outcome.error)
    return errorResponse(ERROR_CODE_HANDLER_ERROR, message)
  }

  // Success: JSON-encode the return value (DECIDED(09)).
  let json: string
  try {
    json = JSON.stringify(outcome.value) ?? "null"
  } catch (error) {
    return errorResponse(
      ERROR_CODE_HANDLER_ERROR,
      `Failed to serialize handler response: ${error instanceof Error ? error.message : String(error)}`,
    )
  }
  return successResponse(new TextEncoder().encode(json))
}

/**
 * Decode command param bytes from an envelope (DECIDED(09) — `ctx.input`):
 * inline base64 → raw bytes; storage → GET the presigned request, use the body
 * bytes. Never JSON-parses (that is the handler's job).
 */
export async function decodeParamsBytes(
  envelope: Envelope,
  fetchImpl: typeof fetch,
): Promise<Uint8Array> {
  const params = envelope.params
  if (params.mode === "inline") {
    return base64ToBytes(params.inlineBase64)
  }

  // Storage mode: download from the presigned request.
  if (!params.storageGetRequest) {
    throw new AlienError(
      ResponseDecodingFailedError.create({
        commandId: envelope.commandId,
        command: envelope.command,
        reason: "Storage params missing storageGetRequest",
      }),
    )
  }

  const request = params.storageGetRequest
  const expiration = new Date(request.expiration)
  if (Date.now() > expiration.getTime()) {
    throw new AlienError(
      StorageOperationFailedError.create({
        operation: "download",
        url: request.backend.type === "http" ? request.backend.url : "local",
        reason: `Presigned request expired at ${expiration.toISOString()}`,
      }),
    )
  }

  if (request.backend.type === "http") {
    const response = await fetchImpl(request.backend.url, {
      method: request.backend.method,
      headers: request.backend.headers,
    })
    if (!response.ok) {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "download",
          url: request.backend.url,
          reason: `HTTP ${response.status} ${response.statusText}`,
        }),
      )
    }
    return new Uint8Array(await response.arrayBuffer())
  }

  // Local backend (dev only).
  const filePath = request.backend.filePath
  if (filePath.includes("..")) {
    throw new AlienError(
      StorageOperationFailedError.create({
        operation: "download",
        url: `local://${filePath}`,
        reason: "Path traversal not allowed in local storage paths",
      }),
    )
  }
  const { readFile } = await import("node:fs/promises")
  const content = await readFile(filePath)
  return new Uint8Array(content)
}

function successResponse(bytes: Uint8Array): CommandResponse {
  return { status: "success", response: bytesToInlineBody(bytes) }
}

function errorResponse(code: string, message: string): CommandResponse {
  return { status: "error", code, message }
}

function bytesToInlineBody(bytes: Uint8Array): BodySpec {
  return { mode: "inline", inlineBase64: bytesToBase64(bytes) }
}

function base64ToBytes(base64: string): Uint8Array {
  return new Uint8Array(Buffer.from(base64, "base64"))
}

function bytesToBase64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64")
}

function logWarn(message: string, error: unknown): void {
  console.warn(`[command-receiver] ${message}: ${describeError(error)}`)
}

function logError(message: string, error: unknown): void {
  console.error(`[command-receiver] ${message}: ${describeError(error)}`)
}

function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
