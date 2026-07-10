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
 * - execution budget = `min(envelope.deadline, leaseExpiresAt − safety margin)`;
 *   on expiry the handler's `signal` fires and a `HANDLER_TIMEOUT` is submitted
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

import { AlienError, LeaseResponseSchema } from "@alienplatform/core"
import {
  CommandReceiverConfigInvalidError,
  InvalidEnvelopeError,
  StorageOperationFailedError,
} from "./errors.js"
import { downloadPresigned, uploadPresigned } from "./presigned.js"
import type {
  BodySpec,
  CommandResponse,
  CommandTargetType,
  Envelope,
  LeaseInfo,
  LeaseRequest,
  PresignedRequest,
} from "./protocol.js"
import { parseWireResponse } from "./wire.js"

/**
 * Presigned-transfer policy for receivers: the local backend is always
 * allowed — receivers run inside the deployment, and the local backend is
 * how the local platform delivers bodies. (Senders gate it behind the
 * `allowLocalStorage` client option instead.)
 */
const RECEIVER_ALLOW_LOCAL = true

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
/**
 * Safety margin subtracted from a lease's expiry when computing the execution
 * budget, in ms. Stopping this far before the lease actually expires
 * guarantees the response is submitted (or the handler abandoned) while the
 * lease is still held, so an expired lease is never redelivered while a
 * duplicate is still in flight. Twin of the Rust receiver's
 * `LEASE_SAFETY_MARGIN` (5s).
 */
const LEASE_SAFETY_MARGIN_MS = 5_000

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
    const endpoint = buildLeaseEndpoint(this.config.url)
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

    const parsed = parseWireResponse(LeaseResponseSchema, await response.json(), "POST", endpoint)
    return parsed.leases
  }

  /**
   * Process one leased command end to end: execute (or reject) it, then submit
   * exactly one response. Only this path submits — a handler cannot double
   * submit, and a submit failure produces no ack, so the lease expires and the
   * command is redelivered.
   */
  private async processLease(lease: LeaseInfo): Promise<void> {
    const response = await this.executeLease(lease)
    const handlerStatus = commandResponseStatus(response)
    let submitStatus: SubmitStatus = "submitted"
    try {
      await this.submitResponse(lease.envelope, response)
    } catch (error) {
      // No ack: the lease will expire and the command is redelivered.
      submitStatus = "failed"
      logError(`Failed to submit response for command '${lease.commandId}'`, error)
    }

    // One structured observability line per command, carrying the pinned
    // receiver fields. Twin of the Rust receiver's `Command processed` event.
    logCommandProcessed({
      commandId: lease.commandId,
      leaseId: lease.leaseId,
      targetResourceId: this.config.resourceId,
      targetResourceType: this.config.resourceType,
      attempt: lease.attempt,
      deadline: lease.envelope.deadline ?? null,
      handlerStatus,
      submitStatus,
    })
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

    const budget = commandBudget(envelope.deadline ?? undefined, leaseExpiresAt)
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
    // it carries no bearer header (matching the Rust twin's submit path). It is
    // rebased onto the receiver's configured commands URL first: the manager
    // mints it from its own base (e.g. `http://localhost:9090`), which is not
    // reachable from behind a container/NAT boundary, while the configured URL
    // is the address the platform already corrected for this network (leases
    // flow through it). The submit endpoint lives on the same manager, so an
    // origin swap preserves the pre-authorized path and response token.
    const submitUrl = rebaseOntoCommandsOrigin(
      envelope.responseHandling.submitResponseUrl,
      this.config.url,
    )
    const res = await this.fetchImpl(submitUrl, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(finalResponse),
    })

    if (!res.ok) {
      const body = await res.text().catch(() => "")
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: submitUrl,
          reason: `Response submission failed with status ${res.status}: ${body}`,
        }),
      )
    }
  }

  private async uploadResponseToStorage(
    request: PresignedRequest,
    bytes: Uint8Array,
  ): Promise<void> {
    await uploadPresigned(request, bytes, {
      fetchImpl: this.fetchImpl,
      allowLocal: RECEIVER_ALLOW_LOCAL,
    })
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
 * Per-command execution budget: `min(envelope.deadline, leaseExpiresAt −
 * LEASE_SAFETY_MARGIN_MS)`, clamped so it never falls before now. There is no
 * lease-renew call, so the safety-margined lease expiry always bounds it. Twin
 * of the Rust receiver's `command_budget`.
 */
export function commandBudget(deadline: string | undefined, leaseExpiresAt: string): Date {
  const leaseBound = Math.max(
    Date.now(),
    new Date(leaseExpiresAt).getTime() - LEASE_SAFETY_MARGIN_MS,
  )
  if (deadline === undefined) {
    return new Date(leaseBound)
  }
  return new Date(Math.min(new Date(deadline).getTime(), leaseBound))
}

/**
 * Build the `/commands/leases` endpoint from the configured base URL.
 *
 * Twin of the Rust receiver's `acquire_leases`, which parses the base as a
 * `Url` and appends segments via `path_segments_mut()` — that mutates only
 * the path, leaving any query string untouched and correctly ordered after
 * the appended segments (M1). The naive string approach this replaced
 * (`url.replace(/\/+$/, "") + "/commands/leases"`) corrupted any base URL
 * carrying a query string, e.g. `https://h/v1?token=x` became
 * `https://h/v1?token=x/commands/leases` instead of
 * `https://h/v1/commands/leases?token=x`.
 *
 * `path_segments_mut()` fails (and the Rust receiver raises
 * `COMMAND_RECEIVER_CONFIG_INVALID`) for a base that cannot be a hierarchical
 * URL — in practice, anything that isn't HTTP(S). This mirrors that check.
 */
export function buildLeaseEndpoint(baseUrl: string): string {
  let url: URL
  try {
    url = new URL(baseUrl)
  } catch {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_URL,
        reason: `${ENV_ALIEN_COMMANDS_URL} '${baseUrl}' must be an HTTP(S) URL with a path`,
      }),
    )
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_URL,
        reason: `${ENV_ALIEN_COMMANDS_URL} '${baseUrl}' must be an HTTP(S) URL with a path`,
      }),
    )
  }

  // `pop_if_empty` equivalent: a trailing slash produces a trailing empty
  // path segment; drop exactly one so we don't double up the separator.
  const segments = url.pathname.split("/")
  if (segments[segments.length - 1] === "") {
    segments.pop()
  }
  segments.push("commands", "leases")
  url.pathname = segments.join("/")
  return url.toString()
}

/**
 * Rebase a manager-minted absolute URL onto the origin of the receiver's
 * configured commands URL, preserving path and query.
 *
 * The manager builds envelope URLs (e.g. the pre-authorized response-submit
 * URL) from its own base address. Across a container or NAT boundary that
 * address may not be reachable from the receiver — the platform corrects
 * `ALIEN_COMMANDS_URL` for the receiver's network (leases already flow
 * through it), so the same origin must be used for every other endpoint on
 * that manager. Returns the URL unchanged when either side fails to parse
 * (an unparseable target fails at fetch time with the real error; the
 * configured base was already validated at receiver construction).
 *
 * Known limitation: only the origin is swapped — a reverse proxy that mounts
 * the manager under a path prefix the manager itself does not know (base
 * `https://edge/prefix/v1` vs minted `…/v1/commands/…`) still breaks, because
 * the prefix cannot be reconstructed client-side. The manager's own base-URL
 * path (e.g. `/v1`) rides inside the minted path and is preserved.
 */
export function rebaseOntoCommandsOrigin(target: string, commandsBaseUrl: string): string {
  try {
    const targetUrl = new URL(target)
    const baseUrl = new URL(commandsBaseUrl)
    if (targetUrl.origin === baseUrl.origin) {
      return target
    }
    return `${baseUrl.origin}${targetUrl.pathname}${targetUrl.search}`
  } catch {
    return target
  }
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
    return decodeInlineParamsBase64(params.inlineBase64)
  }

  // Storage mode: download from the presigned request.
  if (!params.storageGetRequest) {
    throw new AlienError(
      InvalidEnvelopeError.create({
        field: "params.storageGetRequest",
        reason: "Storage params missing storageGetRequest",
      }),
    )
  }

  return downloadPresigned(params.storageGetRequest, {
    fetchImpl,
    allowLocal: RECEIVER_ALLOW_LOCAL,
  })
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

/**
 * Canonical base64 (RFC 4648 §4): 4-char groups from the standard alphabet,
 * with correct padding on the final group. `Buffer.from(str, "base64")` is
 * lenient by design — it silently skips invalid characters and tolerates
 * missing/incorrect padding instead of failing — so untrusted wire input
 * needs this check in front of it to fail loudly instead of decoding to
 * truncated garbage bytes.
 */
const STRICT_BASE64_PATTERN = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/

/**
 * Decode inline command params bytes, matching the Rust twin's strict
 * `base64::engine::general_purpose::STANDARD` decode: any input outside the
 * canonical alphabet/padding fails with `INVALID_ENVELOPE`
 * (DECIDED(09) — twin-pinned; see `PACKAGE_LAYOUT.md`).
 */
function decodeInlineParamsBase64(inlineBase64: string): Uint8Array {
  if (!STRICT_BASE64_PATTERN.test(inlineBase64)) {
    throw new AlienError(
      InvalidEnvelopeError.create({
        field: "params.inlineBase64",
        reason: "Failed to decode base64 params",
      }),
    )
  }
  return base64ToBytes(inlineBase64)
}

function bytesToBase64(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64")
}

/** Submit-response outcome label for the `Command processed` observability line. */
type SubmitStatus = "submitted" | "failed"

/**
 * Handler-status label for a produced response: `"success"` for a success
 * response, otherwise the error code (`UNKNOWN_COMMAND` / `HANDLER_ERROR` /
 * `HANDLER_TIMEOUT` / a params-decode code). Twin of the Rust receiver's
 * `command_response_status`.
 */
function commandResponseStatus(response: CommandResponse): string {
  return response.status === "success" ? "success" : response.code
}

/** The pinned per-command observability fields (twin of the Rust event). */
interface CommandProcessedFields {
  commandId: string
  leaseId: string
  targetResourceId: string
  targetResourceType: CommandTargetType
  attempt: number
  deadline: string | null
  handlerStatus: string
  submitStatus: SubmitStatus
}

function logCommandProcessed(fields: CommandProcessedFields): void {
  console.info(`[command-receiver] Command processed ${JSON.stringify(fields)}`)
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
