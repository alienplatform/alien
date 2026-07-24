/**
 * Pull command receiver — the app-owned lease loop for Containers and Daemons.
 *
 * Behavior-identical twin of the Rust `alien_commands::Receiver`. It leases
 * commands addressed to its own target resource from the command server over
 * outbound HTTPS (no inbound connections), dispatches them to
 * in-process handlers, and submits responses through the envelope's
 * response-handling flow (inline or presigned storage upload).
 *
 * Pure `fetch`; no bindings. The receiver implements these public
 * command-delivery semantics:
 * - required identity plus token-or-token-file env, with fail-fast validation
 * - execution budget = `min(envelope.deadline, leaseExpiresAt − safety margin)`;
 *   on expiry the handler's `signal` fires and a `HANDLER_TIMEOUT` is submitted
 * - error codes `UNKNOWN_COMMAND` / handler `error.code` (falling back to
 *   `HANDLER_ERROR`) / `HANDLER_TIMEOUT`, with params-decode failures retaining
 *   their decode error code
 * - at-least-once delivery; `ctx.attempt` starts at 1
 * - drain: shutdown stops polls, waits up to 30s, then aborts and releases
 * - lease params 5s / maxLeases 1 / leaseSeconds 60, all env-tunable
 * - `ctx.input` = decoded param bytes; success body = `JSON.stringify(return)`;
 *   `ctx.deadline` = the effective budget, always present while a lease is held
 *
 * # Bootstrap
 *
 * ```typescript
 * import { createCommandReceiver } from "@alienplatform/commands"
 *
 * const receiver = createCommandReceiver()
 * receiver.command("generate-report", async params => {
 *   return { report: params }
 * })
 * await receiver.run() // call receiver.stop() to drain and return
 * ```
 */

import { readFile } from "node:fs/promises"
import { AlienError, LeaseResponseSchema } from "@alienplatform/core"
import {
  CommandReceiverConfigInvalidError,
  InvalidEnvelopeError,
  ManagerHttpError,
  StorageOperationFailedError,
} from "./errors.js"
import { downloadPresigned, redactUrlForError, uploadPresigned } from "./presigned.js"
import type {
  BodySpec,
  CommandResponse,
  CommandTargetType,
  Envelope,
  LeaseInfo,
  LeaseRequest,
  PresignedRequest,
  TraceContext,
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

/** Lease poll interval, in ms. */
const DEFAULT_POLL_INTERVAL_MS = 5_000
/** Max leases requested per poll. One process executes one command at a time by default. */
const DEFAULT_MAX_LEASES = 1
/** Requested lease duration, in seconds. */
const DEFAULT_LEASE_SECONDS = 60
/** Maximum interval reached by the empty/error poll backoff. */
const DEFAULT_POLL_MAX_INTERVAL_MS = 30_000
/** Fractional randomization applied to poll sleeps. */
const DEFAULT_POLL_JITTER = 0.1
/** Time allowed for in-flight handlers to finish before abort + release. */
const DEFAULT_DRAIN_TIMEOUT_MS = 30_000
/**
 * Safety margin subtracted from a lease's expiry when computing the execution
 * budget, in ms. Stopping this far before the lease actually expires
 * guarantees the response is submitted (or the handler abandoned) while the
 * lease is still held, so an expired lease is never redelivered while a
 * duplicate is still in flight. Twin of the Rust receiver's
 * `LEASE_SAFETY_MARGIN` (5s).
 */
const LEASE_SAFETY_MARGIN_MS = 5_000
/**
 * Timeout on control-plane HTTP calls (lease acquire, response submit), in
 * ms. `fetch` has no default timeout, so a hung call would otherwise freeze
 * the poll loop indefinitely. Twin of the Rust receiver's 30s reqwest
 * timeout.
 */
const CONTROL_TIMEOUT_MS = 30_000

// Env variable names — identical strings to the Rust twin
// (`alien_core::runtime_environment`).
const ENV_ALIEN_COMMANDS_URL = "ALIEN_COMMANDS_URL"
const ENV_ALIEN_COMMANDS_TOKEN = "ALIEN_COMMANDS_TOKEN"
const ENV_ALIEN_COMMANDS_TOKEN_FILE = "ALIEN_COMMANDS_TOKEN_FILE"
const ENV_ALIEN_DEPLOYMENT_ID = "ALIEN_DEPLOYMENT_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID = "ALIEN_COMMANDS_TARGET_RESOURCE_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE = "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE"
const ENV_ALIEN_COMMANDS_LEASE_SECONDS = "ALIEN_COMMANDS_LEASE_SECONDS"
const ENV_ALIEN_COMMANDS_MAX_LEASES = "ALIEN_COMMANDS_MAX_LEASES"
const ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS = "ALIEN_COMMANDS_POLL_INTERVAL_MS"
const ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS = "ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS"
const ENV_ALIEN_COMMANDS_POLL_JITTER = "ALIEN_COMMANDS_POLL_JITTER"
const ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS = "ALIEN_COMMANDS_DRAIN_TIMEOUT_MS"

/**
 * Per-command context passed to a {@link CommandHandler}.
 *
 * These fields match the Rust `Context` struct
 * (`input`/`deadline`/`command_id`/`attempt`/`cancellation`), the last mapping
 * to `signal` here.
 */
export interface CommandContext {
  /**
   * Decoded command param bytes — the same bytes the params envelope carries
   * after decode, prior to any handler-side parsing. This is byte-for-byte
   * identical to the Rust receiver's `ctx.input`.
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
   * Always present while a lease is held.
   */
  deadline: Date
  /** Unique command identifier. */
  commandId: string
  /** The resource identity this receiver owns. */
  target: {
    resourceId: string
    resourceType: Exclude<CommandTargetType, "worker">
  }
  /** Optional W3C trace context propagated from the command envelope. */
  traceContext?: TraceContext
  /**
   * Delivery attempt, starting at 1. Greater than 1 means redelivery
   * (at-least-once semantics); handlers must tolerate running more than once.
   */
  attempt: number
}

/**
 * A command handler. Receives a {@link CommandContext} and returns any
 * JSON-serializable value, submitted as the command's success response
 * (`JSON.stringify`-encoded). Throwing/rejecting submits the error's non-empty
 * string `code` when present, otherwise `HANDLER_ERROR`.
 */
export type RawCommandHandler = (ctx: CommandContext) => unknown | Promise<unknown>

/** The part of Standard Schema v1 used to validate command inputs. */
export interface StandardSchema<Input = unknown, Output = Input> {
  readonly "~standard": {
    readonly version: 1
    readonly vendor: string
    readonly validate: (
      value: unknown,
    ) =>
      | { readonly value: Output; readonly issues?: undefined }
      | { readonly issues: ReadonlyArray<{ readonly message: string }> }
      | Promise<
          | { readonly value: Output; readonly issues?: undefined }
          | { readonly issues: ReadonlyArray<{ readonly message: string }> }
        >
    readonly types?: { readonly input: Input; readonly output: Output }
  }
}

/** Infer the validated output type of a Standard Schema. */
export type StandardSchemaOutput<Schema extends StandardSchema> = NonNullable<
  Schema["~standard"]["types"]
>["output"]

/** A JSON command handler. */
export type CommandHandler<Input = unknown> = (
  input: Input,
  context: CommandContext,
) => unknown | Promise<unknown>

/**
 * The pull receiver handle. Register handlers with {@link CommandReceiver.command},
 * drive the lease loop with {@link CommandReceiver.run}, and stop it with
 * {@link CommandReceiver.stop}.
 */
export interface CommandReceiver {
  /** Register a schema-less JSON command. Its input is deliberately `unknown`. */
  command(name: string, handler: CommandHandler<unknown>): CommandReceiver
  /** Register a JSON command validated by a Standard Schema v1 validator. */
  command<Schema extends StandardSchema>(
    name: string,
    schema: Schema,
    handler: CommandHandler<StandardSchemaOutput<Schema>>,
  ): CommandReceiver
  /** Register an advanced handler that receives the encoded input bytes. */
  handleRaw(name: string, handler: RawCommandHandler): CommandReceiver
  /**
   * Drive the lease loop until {@link CommandReceiver.stop} is called. No new
   * lease poll *starts* once draining begins; a poll already in flight
   * completes and its leases are dispatched. In-flight commands may finish
   * within the drain timeout; remaining handlers are aborted and released.
   * Retryable or unknown transport failures continue polling; non-retryable
   * {@link AlienError}s terminate the receiver after draining.
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
  /** Lease poll interval in ms (default 5000). Overrides the environment. */
  pollIntervalMs?: number
  /** Maximum empty/error poll interval in ms (default 30000). */
  pollMaxIntervalMs?: number
  /** Poll jitter fraction from 0 to 1 (default 0.1). */
  pollJitter?: number
  /** Requested lease duration in seconds (default 60). Overrides the environment. */
  leaseSeconds?: number
  /** Max leases requested per poll (default 1). Overrides the environment. */
  maxLeases?: number
  /** Graceful drain timeout in ms (default 30000). */
  drainTimeoutMs?: number
}

interface ReceiverConfig {
  url: string
  token?: string
  tokenFile?: string
  deploymentId: string
  resourceId: string
  resourceType: Exclude<CommandTargetType, "worker">
  pollIntervalMs: number
  pollMaxIntervalMs: number
  pollJitter: number
  leaseSeconds: number
  maxLeases: number
  drainTimeoutMs: number
}

/**
 * Construct the pull receiver from environment configuration.
 *
 * Validates required identity, token-source selection, URL parseability, and
 * numeric tuning values **synchronously**. An invalid value throws
 * {@link CommandReceiverConfigInvalidError} naming the offending variable in
 * `context.envVar`. Token-file contents and the HTTP(S) URL requirement are
 * validated asynchronously when `run` starts. `resourceType` must be
 * `container` or `daemon`; `worker` (and anything else) is rejected — a
 * receiver must not guess its target type.
 */
export function createCommandReceiver(options: CommandReceiverOptions = {}): CommandReceiver {
  const env = options.env ?? (typeof process !== "undefined" ? process.env : {})
  const config = validateConfig(env, options)
  return new PullCommandReceiver(config, options)
}

function requireEnv(env: Record<string, string | undefined>, name: string): string {
  const value = env[name]
  if (value === undefined) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({ envVar: name, reason: `${name} is required` }),
    )
  }
  if (value.trim() === "") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: name,
        reason: `${name} must not be empty`,
      }),
    )
  }
  return value
}

function optionalNonEmpty(
  env: Record<string, string | undefined>,
  name: string,
): string | undefined {
  const value = env[name]
  if (value?.trim() === "") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: name,
        reason: `${name} must not be empty`,
      }),
    )
  }
  return value
}

function numericConfig(
  env: Record<string, string | undefined>,
  envName: string,
  override: number | undefined,
  fallback: number,
  validate: (value: number) => boolean,
): number {
  const raw = override ?? (env[envName] === undefined ? fallback : Number(env[envName]))
  if (!Number.isFinite(raw) || !validate(raw)) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: envName,
        reason: `${envName} has invalid numeric value '${env[envName] ?? raw}'`,
      }),
    )
  }
  return raw
}

function validateConfig(
  env: Record<string, string | undefined>,
  options: CommandReceiverOptions,
): ReceiverConfig {
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

  const token = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN)
  const tokenFile = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN_FILE)
  if (token === undefined && tokenFile === undefined) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_TOKEN,
        reason: `${ENV_ALIEN_COMMANDS_TOKEN} or ${ENV_ALIEN_COMMANDS_TOKEN_FILE} is required`,
      }),
    )
  }
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

  const pollIntervalMs = numericConfig(
    env,
    ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS,
    options.pollIntervalMs,
    DEFAULT_POLL_INTERVAL_MS,
    value => Number.isInteger(value) && value > 0,
  )
  const pollMaxIntervalMs = numericConfig(
    env,
    ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
    options.pollMaxIntervalMs,
    DEFAULT_POLL_MAX_INTERVAL_MS,
    value => Number.isInteger(value) && value > 0,
  )
  if (pollMaxIntervalMs < pollIntervalMs) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
        reason: `${ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS} must be at least ${ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS}`,
      }),
    )
  }

  return {
    url,
    token,
    tokenFile,
    deploymentId,
    resourceId,
    resourceType: rawType,
    pollIntervalMs,
    pollMaxIntervalMs,
    pollJitter: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_POLL_JITTER,
      options.pollJitter,
      DEFAULT_POLL_JITTER,
      value => value >= 0 && value <= 1,
    ),
    leaseSeconds: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_LEASE_SECONDS,
      options.leaseSeconds,
      DEFAULT_LEASE_SECONDS,
      value => Number.isInteger(value) && value > 0,
    ),
    maxLeases: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_MAX_LEASES,
      options.maxLeases,
      DEFAULT_MAX_LEASES,
      value => Number.isInteger(value) && value > 0,
    ),
    drainTimeoutMs: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS,
      options.drainTimeoutMs,
      DEFAULT_DRAIN_TIMEOUT_MS,
      value => Number.isInteger(value) && value >= 0,
    ),
  }
}

class TokenSource {
  private cachedFileToken: string | undefined

  constructor(
    private readonly token: string | undefined,
    private readonly tokenFile: string | undefined,
  ) {}

  get refreshable(): boolean {
    return this.token === undefined && this.tokenFile !== undefined
  }

  async read(forceRefresh = false): Promise<string> {
    if (this.token !== undefined) return this.token
    if (!forceRefresh && this.cachedFileToken !== undefined) return this.cachedFileToken

    const path = this.tokenFile as string
    let token: string
    try {
      token = (await readFile(path, "utf8")).trim()
    } catch (error) {
      throw (await AlienError.from(error)).withContext(
        CommandReceiverConfigInvalidError.create({
          envVar: ENV_ALIEN_COMMANDS_TOKEN_FILE,
          reason: `Failed to read command token file '${path}'`,
        }),
      )
    }
    if (token === "") {
      throw new AlienError(
        CommandReceiverConfigInvalidError.create({
          envVar: ENV_ALIEN_COMMANDS_TOKEN_FILE,
          reason: `${ENV_ALIEN_COMMANDS_TOKEN_FILE} '${path}' contains an empty token`,
        }),
      )
    }
    this.cachedFileToken = token
    return token
  }
}

interface ActiveLease {
  lease: LeaseInfo
  controller: AbortController
  task: Promise<void>
}

class PullCommandReceiver implements CommandReceiver {
  private readonly config: ReceiverConfig
  private readonly fetchImpl: typeof fetch
  private readonly pollIntervalMs: number
  private readonly leaseSeconds: number
  private readonly maxLeases: number
  private readonly pollMaxIntervalMs: number
  private readonly pollJitter: number
  private readonly drainTimeoutMs: number
  private readonly tokenSource: TokenSource
  private readonly handlers = new Map<string, RawCommandHandler>()
  private readonly shutdown = new AbortController()
  private readonly active = new Map<string, ActiveLease>()

  constructor(config: ReceiverConfig, options: CommandReceiverOptions) {
    this.config = config
    this.fetchImpl = options.fetch ?? globalThis.fetch
    this.pollIntervalMs = config.pollIntervalMs
    this.pollMaxIntervalMs = config.pollMaxIntervalMs
    this.pollJitter = config.pollJitter
    this.leaseSeconds = config.leaseSeconds
    this.maxLeases = config.maxLeases
    this.drainTimeoutMs = config.drainTimeoutMs
    this.tokenSource = new TokenSource(config.token, config.tokenFile)
  }

  command(name: string, handler: CommandHandler<unknown>): CommandReceiver
  command<Schema extends StandardSchema>(
    name: string,
    schema: Schema,
    handler: CommandHandler<StandardSchemaOutput<Schema>>,
  ): CommandReceiver
  command<Schema extends StandardSchema>(
    name: string,
    schemaOrHandler: Schema | CommandHandler<unknown>,
    validatedHandler?: CommandHandler<StandardSchemaOutput<Schema>>,
  ): CommandReceiver {
    const schema = validatedHandler === undefined ? undefined : (schemaOrHandler as Schema)
    const handler = (validatedHandler ?? schemaOrHandler) as CommandHandler<unknown>
    return this.handleRaw(name, async context => {
      let input: unknown
      try {
        input = JSON.parse(new TextDecoder().decode(context.input))
      } catch {
        throw new Error("Command input is not valid JSON")
      }
      if (schema !== undefined) {
        const result = await schema["~standard"].validate(input)
        if (result.issues !== undefined) {
          const details = result.issues.map(issue => issue.message).join("; ")
          throw new Error(`Command input failed validation${details ? `: ${details}` : ""}`)
        }
        input = result.value
      }
      return await handler(input, context)
    })
  }

  handleRaw(name: string, handler: RawCommandHandler): CommandReceiver {
    this.handlers.set(name, handler)
    return this
  }

  stop(): void {
    this.shutdown.abort()
  }

  async run(): Promise<void> {
    const inFlight = new Set<Promise<void>>()
    let nextPollMs = this.pollIntervalMs
    let terminalError: AlienError | undefined

    // Mirrors the Rust run loop: check shutdown at the top of each iteration
    // (no new poll starts once draining begins), acquire leases (a poll already
    // in flight completes and its leases are dispatched), then sleep-or-stop.
    while (!this.shutdown.signal.aborted) {
      let leases: LeaseInfo[] = []
      let sleepMs = nextPollMs
      const available = Math.max(0, this.maxLeases - this.active.size)
      if (available === 0) {
        sleepMs = this.pollIntervalMs
        nextPollMs = this.pollIntervalMs
      } else {
        try {
          leases = await this.acquireLeases(available)
          if (leases.length > 0) {
            sleepMs = this.pollIntervalMs
            nextPollMs = this.pollIntervalMs
          } else {
            nextPollMs = this.nextBackoff(nextPollMs)
          }
        } catch (error) {
          if (error instanceof AlienError && !error.retryable) {
            terminalError = error
            break
          }
          // Retryable Alien errors and unknown transport errors are logged and
          // retried next interval.
          logWarn("Failed to acquire command leases, will retry", error)
          nextPollMs = this.nextBackoff(nextPollMs)
        }
      }

      for (const lease of leases) {
        if (this.active.has(lease.commandId)) {
          await this.releaseLease(lease.leaseId)
          continue
        }
        const controller = new AbortController()
        const task = this.processLease(lease, controller).finally(() => {
          const current = this.active.get(lease.commandId)
          if (current?.lease.leaseId === lease.leaseId) this.active.delete(lease.commandId)
        })
        this.active.set(lease.commandId, { lease, controller, task })
        inFlight.add(task)
        void task.finally(() => inFlight.delete(task))
      }

      if (this.shutdown.signal.aborted) {
        break
      }
      await this.sleepOrStop(this.withJitter(sleepMs))
    }

    let drainTimer: ReturnType<typeof setTimeout> | undefined
    const drained = await Promise.race([
      Promise.all([...inFlight]).then(() => true),
      new Promise<false>(resolve => {
        drainTimer = setTimeout(() => resolve(false), this.drainTimeoutMs)
      }),
    ])
    if (drainTimer !== undefined) clearTimeout(drainTimer)
    if (!drained) {
      for (const active of this.active.values()) active.controller.abort()
    }
    await Promise.all([...inFlight])
    if (terminalError !== undefined) throw terminalError
  }

  private nextBackoff(current: number): number {
    return Math.min(this.pollMaxIntervalMs, Math.max(this.pollIntervalMs, current * 2))
  }

  private withJitter(ms: number): number {
    const factor = 1 + (Math.random() * 2 - 1) * this.pollJitter
    return Math.max(0, Math.round(ms * factor))
  }

  /** Build the lease request this receiver sends (pure — unit-testable). */
  private buildLeaseRequest(maxLeases: number): LeaseRequest {
    return {
      deploymentId: this.config.deploymentId,
      target: {
        resourceId: this.config.resourceId,
        resourceType: this.config.resourceType,
      },
      maxLeases,
      leaseSeconds: this.leaseSeconds,
    }
  }

  private async acquireLeases(maxLeases: number): Promise<LeaseInfo[]> {
    const endpoint = buildLeaseEndpoint(this.config.url)
    const response = await this.authenticatedFetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(this.buildLeaseRequest(maxLeases)),
      // fetch has no default timeout; a hung lease call would freeze the
      // whole poll loop, so cap it well under the lease duration.
      signal: AbortSignal.timeout(CONTROL_TIMEOUT_MS),
    })

    if (!response.ok) {
      const context = {
        method: "POST",
        url: endpoint,
        status: response.status,
        statusText: response.statusText,
      }
      const definition = ManagerHttpError.create(context)
      throw new AlienError({
        code: definition.metadata.code,
        message: definition.metadata.message(context),
        retryable: response.status === 408 || response.status === 429 || response.status >= 500,
        internal: definition.metadata.internal,
        httpStatusCode: definition.metadata.httpStatusCode,
        context,
      })
    }

    const parsed = parseWireResponse(LeaseResponseSchema, await response.json(), "POST", endpoint)
    // Lease-served envelopes carry manager URLs relative to the exact lease
    // endpoint. Resolving against the endpoint that succeeded preserves both
    // its reachable origin and any reverse-proxy path prefix. Absolute cloud-
    // presigned storage URLs pass through byte-for-byte.
    for (const lease of parsed.leases) {
      resolveEnvelopeUrls(lease.envelope, endpoint)
    }
    return parsed.leases
  }

  private async authenticatedFetch(endpoint: string, init: RequestInit): Promise<Response> {
    const send = async (forceRefresh: boolean) => {
      const token = await this.tokenSource.read(forceRefresh)
      const headers = new Headers(init.headers)
      headers.set("Authorization", `Bearer ${token}`)
      return this.fetchImpl(endpoint, { ...init, headers })
    }
    let response = await send(false)
    if (response.status === 401 && this.tokenSource.refreshable) {
      response = await send(true)
    }
    return response
  }

  /**
   * Process one leased command end to end: execute (or reject) it, then submit
   * exactly one response. Only this path submits — a handler cannot double
   * submit, and a submit failure produces no ack, so the lease expires and the
   * command is redelivered.
   */
  private async processLease(lease: LeaseInfo, controller: AbortController): Promise<void> {
    const executionBudget = commandBudget(
      lease.envelope.deadline ?? undefined,
      lease.leaseExpiresAt,
    )
    const response = await this.executeLease(lease, controller, executionBudget)
    if (response === undefined) {
      await this.releaseLease(lease.leaseId)
      return
    }
    const handlerStatus = commandResponseStatus(response)
    let submitStatus: SubmitStatus = "submitted"
    try {
      await this.submitResponse(lease.envelope, response, lease.leaseExpiresAt)
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
  private async executeLease(
    lease: LeaseInfo,
    controller: AbortController,
    budget: Date,
  ): Promise<CommandResponse | undefined> {
    const { envelope, attempt } = lease
    const handler = this.handlers.get(envelope.command)
    if (!handler) {
      return errorResponse(
        ERROR_CODE_UNKNOWN_COMMAND,
        `No handler registered for command '${envelope.command}'`,
      )
    }

    const execute = async (): Promise<CommandResponse> => {
      let input: Uint8Array
      try {
        input = await decodeParamsBytes(envelope, this.fetchImpl, controller.signal)
      } catch (error) {
        // Decode failure is submitted under the decode error's own code, not a
        // receiver-specific one.
        const code = error instanceof AlienError ? error.code : ERROR_CODE_HANDLER_ERROR
        return errorResponse(code, error instanceof Error ? error.message : String(error))
      }

      const ctx: CommandContext = {
        input,
        signal: controller.signal,
        deadline: budget,
        commandId: envelope.commandId,
        attempt,
        target: {
          resourceId: this.config.resourceId,
          resourceType: this.config.resourceType,
        },
        traceContext: envelope.traceContext ?? undefined,
      }
      return invokeHandler(handler, ctx)
    }

    return runUnderBudget(execute, budget, controller, envelope.command)
  }

  private async releaseLease(leaseId: string): Promise<void> {
    const endpoint = buildReleaseEndpoint(this.config.url, leaseId)
    try {
      const response = await this.authenticatedFetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ leaseId }),
        signal: AbortSignal.timeout(CONTROL_TIMEOUT_MS),
      })
      if (response.ok || response.status === 409 || response.status === 410) return
      const body = await response.text().catch(() => "")
      logWarn(
        `Failed to release command lease '${leaseId}' (${response.status}: ${body})`,
        response,
      )
    } catch (error) {
      logWarn(`Failed to release command lease '${leaseId}'`, error)
    }
  }

  private async submitResponse(
    envelope: Envelope,
    response: CommandResponse,
    leaseExpiresAt: string,
  ): Promise<void> {
    const remainingLeaseMs = new Date(leaseExpiresAt).getTime() - Date.now()
    if (remainingLeaseMs <= 0) {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: redactUrlForError(envelope.responseHandling.submitResponseUrl),
          reason: "Lease expired before response submission could start",
        }),
      )
    }
    // One signal covers storage overflow plus the final status PUT. Reusing it
    // prevents either stage from starting a fresh timeout and extending the
    // lease. The normal 30-second control cap still applies.
    const submissionTimeoutMs = Math.min(CONTROL_TIMEOUT_MS, remainingLeaseMs)
    const submissionSignal = AbortSignal.timeout(submissionTimeoutMs)
    let finalResponse = response

    if (response.status === "success" && response.response.mode === "inline") {
      const bytes = base64ToBytes(response.response.inlineBase64)
      const maxInline = envelope.responseHandling.maxInlineBytes
      if (bytes.byteLength > maxInline) {
        // Large response: upload to storage first, then reference it.
        await this.uploadResponseToStorage(
          envelope.responseHandling.storageUploadRequest,
          bytes,
          submissionSignal,
        )
        finalResponse = {
          status: "success",
          response: { mode: "storage", size: bytes.byteLength, storagePutUsed: true },
        }
      }
    }

    // Lease ingestion resolves the pre-authorized submit reference against the
    // exact endpoint used to acquire it. It is fully qualified here and carries
    // no bearer header, matching the Rust twin's submit path.
    const submitUrl = envelope.responseHandling.submitResponseUrl
    let res: Response
    try {
      res = await this.fetchImpl(submitUrl, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(finalResponse),
        signal: submissionSignal,
      })
    } catch (error) {
      throw (await AlienError.from(error)).withContext(
        StorageOperationFailedError.create({
          operation: "upload",
          url: redactUrlForError(submitUrl),
          reason: describeResponseSubmissionFailure(error, submissionSignal, submissionTimeoutMs),
        }),
      )
    }

    if (!res.ok && res.status !== 409 && res.status !== 410) {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: redactUrlForError(submitUrl),
          // A failing endpoint may echo the bearer-equivalent response token
          // or another signed URL, so its body is not diagnostic-safe.
          reason: `Response submission failed with status ${res.status}`,
        }),
      )
    }
  }

  private async uploadResponseToStorage(
    request: PresignedRequest,
    bytes: Uint8Array,
    signal: AbortSignal,
  ): Promise<void> {
    await uploadPresigned(request, bytes, {
      fetchImpl: this.fetchImpl,
      allowLocal: RECEIVER_ALLOW_LOCAL,
      signal,
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
 * LEASE_SAFETY_MARGIN_MS)`. The LEASE bound is clamped to now; an
 * already-past deadline is not — it yields a zero budget and an immediate
 * HANDLER_TIMEOUT, the correct outcome for a command delivered after its
 * deadline. There is no
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

/** Build the authenticated lease-release endpoint without corrupting base queries. */
export function buildReleaseEndpoint(baseUrl: string, leaseId: string): string {
  const url = new URL(buildLeaseEndpoint(baseUrl))
  const segments = url.pathname.split("/")
  segments.push(encodeURIComponent(leaseId), "release")
  url.pathname = segments.join("/")
  return url.toString()
}

/**
 * Resolve path-relative envelope URLs in place against the exact lease
 * endpoint used to acquire them.
 *
 * The manager cannot know an address reachable from behind every consumer's
 * network boundary or a reverse-proxy prefix added outside the manager. The
 * successful lease endpoint contains both. Absolute cloud-presigned storage
 * URLs pass through byte-for-byte. Twin of Rust `resolve_envelope_urls`.
 */
export function resolveEnvelopeUrls(envelope: Envelope, leaseEndpoint: string): void {
  let base: URL
  try {
    base = new URL(leaseEndpoint)
  } catch {
    // Unparseable base (already rejected at construction): leave the
    // envelope as served.
    return
  }
  const resolve = (target: string) => {
    if (target.startsWith("//")) return target
    try {
      new URL(target)
      return target
    } catch {
      try {
        return new URL(target, base).toString()
      } catch {
        return target
      }
    }
  }

  envelope.responseHandling.submitResponseUrl = resolve(envelope.responseHandling.submitResponseUrl)
  const upload = envelope.responseHandling.storageUploadRequest
  if (upload.backend.type === "http") {
    upload.backend.url = resolve(upload.backend.url)
  }
  if (
    envelope.params.mode === "storage" &&
    envelope.params.storageGetRequest?.backend.type === "http"
  ) {
    envelope.params.storageGetRequest.backend.url = resolve(
      envelope.params.storageGetRequest.backend.url,
    )
  }
}

/**
 * Run params decoding plus the handler under one budget timer. On budget
 * expiry the `signal` fires, the operation promise is abandoned (its later
 * settlement is ignored), and a `HANDLER_TIMEOUT` is returned.
 */
async function runUnderBudget(
  operation: () => Promise<CommandResponse>,
  budget: Date,
  controller: AbortController,
  command: string,
): Promise<CommandResponse | undefined> {
  const remainingMs = Math.max(0, budget.getTime() - Date.now())
  if (remainingMs === 0) {
    controller.abort()
    return handlerTimeoutResponse(command, budget)
  }

  let timer: ReturnType<typeof setTimeout> | undefined
  const budgetPromise = new Promise<{ kind: "timeout" }>(resolve => {
    timer = setTimeout(() => resolve({ kind: "timeout" }), remainingMs)
  })

  const operationPromise = Promise.resolve()
    .then(operation)
    .then(
      response => ({ kind: "return" as const, response }),
      (error: unknown) => ({ kind: "throw" as const, error }),
    )

  const shutdownPromise = new Promise<{ kind: "shutdown" }>(resolve => {
    if (controller.signal.aborted) {
      resolve({ kind: "shutdown" })
      return
    }
    controller.signal.addEventListener("abort", () => resolve({ kind: "shutdown" }), {
      once: true,
    })
  })

  const outcome = await Promise.race([operationPromise, budgetPromise, shutdownPromise])
  if (timer !== undefined) {
    clearTimeout(timer)
  }

  if (outcome.kind === "timeout") {
    // Budget expired: fire the signal for cooperative work; abandon the handler
    // promise so a late settlement can't double-submit.
    controller.abort()
    void operationPromise.catch(() => {})
    return handlerTimeoutResponse(command, budget)
  }

  if (outcome.kind === "shutdown") {
    void operationPromise.catch(() => {})
    return undefined
  }

  if (outcome.kind === "throw") {
    const message = outcome.error instanceof Error ? outcome.error.message : String(outcome.error)
    return errorResponse(handlerErrorCode(outcome.error), message)
  }

  return outcome.response
}

function handlerTimeoutResponse(command: string, budget: Date): CommandResponse {
  return errorResponse(
    ERROR_CODE_HANDLER_TIMEOUT,
    `Command '${command}' exceeded its execution budget (${budget.toISOString()})`,
  )
}

/** Invoke and JSON-encode a handler without creating a new execution budget. */
async function invokeHandler(
  handler: RawCommandHandler,
  ctx: CommandContext,
): Promise<CommandResponse> {
  let value: unknown
  try {
    value = await handler(ctx)
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    return errorResponse(handlerErrorCode(error), message)
  }

  // Success: JSON-encode the return value.
  let json: string
  try {
    json = JSON.stringify(value) ?? "null"
  } catch (error) {
    return errorResponse(
      ERROR_CODE_HANDLER_ERROR,
      `Failed to serialize handler response: ${error instanceof Error ? error.message : String(error)}`,
    )
  }
  return successResponse(new TextEncoder().encode(json))
}

function handlerErrorCode(error: unknown): string {
  if (typeof error !== "object" || error === null || !("code" in error)) {
    return ERROR_CODE_HANDLER_ERROR
  }
  const code = error.code
  return typeof code === "string" && code.length > 0 ? code : ERROR_CODE_HANDLER_ERROR
}

/**
 * Decode command param bytes for `ctx.input`: inline base64 → raw bytes;
 * storage → GET the presigned request and use the body bytes. Never JSON-parses
 * (that is the handler's job).
 */
export async function decodeParamsBytes(
  envelope: Envelope,
  fetchImpl: typeof fetch,
  signal?: AbortSignal,
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
    signal,
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
 * canonical alphabet/padding fails with `INVALID_ENVELOPE`.
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
 * response, otherwise the error code (`UNKNOWN_COMMAND` / a handler-provided
 * code or `HANDLER_ERROR` / `HANDLER_TIMEOUT` / a params-decode code). Twin of
 * the Rust receiver's `command_response_status`.
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

function describeResponseSubmissionFailure(
  error: unknown,
  signal: AbortSignal,
  timeoutMs: number,
): string {
  if (signal.aborted) {
    return `Response submission timed out after ${timeoutMs}ms before a response was received`
  }

  const details = safeTransportErrorDetails(error)
  return details === undefined
    ? "Response submission failed before a response was received"
    : `Response submission failed before a response was received (${details})`
}

function safeTransportErrorDetails(error: unknown): string | undefined {
  if (!(error instanceof Error)) return undefined

  const name = safeDiagnosticToken(error.name)
  const causeCode =
    typeof error.cause === "object" && error.cause !== null && "code" in error.cause
      ? safeDiagnosticToken(error.cause.code)
      : undefined

  if (name !== undefined && causeCode !== undefined) return `${name}; cause=${causeCode}`
  return name
}

function safeDiagnosticToken(value: unknown): string | undefined {
  return typeof value === "string" && /^[A-Za-z0-9_.-]{1,64}$/.test(value) ? value : undefined
}
