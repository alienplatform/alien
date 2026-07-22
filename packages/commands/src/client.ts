/**
 * Command sender — invoke commands on remote Alien deployments.
 *
 * Migrated from the former `@alienplatform/sdk/commands` subpath. Pure `fetch`; no
 * bindings. Handles:
 * - Base64 encoding/decoding of input and responses
 * - Large payload download via storage presigned transfers
 * - Polling with exponential backoff
 * - Error handling and timeout
 *
 * `.target(name)` returns a {@link TargetedCommands} bound sender that presets
 * `targetResourceId` on every invoke — mirroring the Rust
 * `CommandsClient::target(...)` builder. The builder's target wins over any
 * `options.targetResourceId` the caller also passes.
 */

import {
  AlienError,
  CommandStatusResponseSchema,
  CreateCommandResponseSchema,
} from "@alienplatform/core"
import {
  CommandCreationFailedError,
  CommandExpiredError,
  CommandStatusFailedError,
  CommandTimeoutError,
  DeploymentCommandError,
  ManagerHttpError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"
import { downloadPresigned, redactUrlForError } from "./presigned.js"
import type {
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CreateCommandResponse,
} from "./protocol.js"
import { type WireSchema, parseWireResponse } from "./wire.js"

/**
 * Configuration for {@link CommandsClient}.
 */
export interface CommandsClientConfig {
  /** Manager URL (e.g. "https://manager.example.com"). Trailing slashes are stripped. */
  managerUrl: string
  /** Deployment ID to invoke commands on. */
  deploymentId: string
  /** Bearer token (deployment token or workspace token). */
  token: string
  /** Default invoke timeout in milliseconds (default: 60000). */
  timeoutMs?: number
  /** Allow reading local files for storage responses (default: false, local dev only). */
  allowLocalStorage?: boolean
  /** `fetch` implementation (defaults to the global `fetch`). */
  fetch?: typeof fetch
}

/**
 * Per-invoke options.
 */
export interface InvokeOptions {
  /** Wall-clock timeout in milliseconds (default: the client's `timeoutMs`). */
  timeoutMs?: number
  /** Optional server-side deadline for command completion. */
  deadline?: Date
  /** Optional idempotency key — the server dedupes retried creates by this key. */
  idempotencyKey?: string
  /** Target command-capable resource id; a `.target(name)` builder overrides this. */
  targetResourceId?: string
  /** Initial polling interval in milliseconds (default: 500). */
  pollIntervalMs?: number
  /** Maximum polling interval in milliseconds (default: 5000). */
  maxPollIntervalMs?: number
  /** Polling backoff multiplier (default: 1.5). */
  pollBackoff?: number
}

/**
 * Serialize `data` as JSON and
 * base64-encode it. This is the one place the send-side serialize decision
 * lives. `@alienplatform/commands` is a Node-only package — the receiver decodes
 * with `Buffer` too — so this uses `Buffer` directly rather than branching on a
 * browser `btoa`.
 */
function base64Encode(data: unknown): string {
  const json = JSON.stringify(data)
  if (json === undefined) {
    throw new TypeError("Command input must be JSON-serializable")
  }
  return Buffer.from(json, "utf-8").toString("base64")
}

/**
 * Base64 decode data. `@alienplatform/commands` is a Node-only package (the
 * send-side encoder uses `Buffer` too), so this decodes with `Buffer` directly
 * rather than branching on a browser `atob`, which mangles multibyte UTF-8.
 */
function base64Decode(encoded: string): string {
  return Buffer.from(encoded, "base64").toString("utf-8")
}

/**
 * Create an inline body spec. Always sends inline — the server handles storage
 * decisions transparently (auto-promoting to blob if needed).
 */
function createBodySpec(data: unknown): BodySpec {
  return {
    mode: "inline",
    inlineBase64: base64Encode(data),
  }
}

/**
 * Decode a response body spec into the handler's return value.
 */
async function decodeBodySpec(
  body: BodySpec,
  commandId: string,
  command: string,
  allowLocalStorage: boolean,
): Promise<unknown> {
  if (body.mode === "inline") {
    const json = base64Decode(body.inlineBase64)
    return JSON.parse(json)
  }

  if (body.mode === "storage") {
    // Storage mode - download from presigned URL
    if (!body.storageGetRequest) {
      throw new AlienError(
        ResponseDecodingFailedError.create({
          commandId,
          command,
          reason: "Storage response missing storageGetRequest",
        }),
      )
    }

    const request = body.storageGetRequest
    const url =
      request.backend.type === "http"
        ? redactUrlForError(request.backend.url)
        : `local://${request.backend.filePath}`

    try {
      // POLICY: the sender only touches the local (dev-only) backend when the
      // client was configured with `allowLocalStorage: true`.
      const bytes = await downloadPresigned(request, { allowLocal: allowLocalStorage })
      return JSON.parse(new TextDecoder().decode(bytes))
    } catch (error) {
      if (error instanceof AlienError) {
        throw error
      }

      // Wrap fetch/filesystem/parse errors
      const alienError = await AlienError.from(error)
      throw alienError.withContext(
        StorageOperationFailedError.create({
          operation: "download",
          url,
          reason: error instanceof Error ? error.message : String(error),
        }),
      )
    }
  }

  throw new AlienError(
    ResponseDecodingFailedError.create({
      commandId,
      command,
      reason: `Unknown body mode: ${(body as { mode: string }).mode}`,
    }),
  )
}

/**
 * Check if a state is terminal.
 */
function isTerminalState(state: CommandState): boolean {
  return state === "SUCCEEDED" || state === "FAILED" || state === "EXPIRED"
}

/**
 * Command sender for invoking deployment commands.
 */
export class CommandsClient {
  private readonly managerUrl: string
  private readonly deploymentId: string
  private readonly token: string
  private readonly defaultTimeout: number
  private readonly allowLocalStorage: boolean
  private readonly fetchImpl: typeof fetch

  constructor(config: CommandsClientConfig) {
    // Store the base URL raw; request URLs are built with URL-based
    // construction in `buildManagerUrl`, which preserves any query string.
    this.managerUrl = config.managerUrl
    this.deploymentId = config.deploymentId
    this.token = config.token
    this.defaultTimeout = config.timeoutMs ?? 60_000
    this.allowLocalStorage = config.allowLocalStorage ?? false
    this.fetchImpl = config.fetch ?? globalThis.fetch
  }

  /**
   * Scope this client to one target command-capable resource: every `invoke`
   * made through the returned builder presets `targetResourceId` to `name`,
   * mirroring the Rust `CommandsClient::target(...)` shorthand.
   *
   * If the caller also passes `options.targetResourceId`, the builder's target
   * silently wins — passing two different targets is a programmer error, not a
   * runtime conflict this builder tries to detect.
   */
  target(name: string): TargetedCommands {
    return new TargetedCommands(this, name)
  }

  /**
   * Invoke a command on the deployment and wait for its response.
   *
   * @param command - Command name (e.g. "generate-report").
   * @param input - Command input (JSON-serializable).
   * @param options - Invocation options.
   * @returns Decoded response data.
   */
  async invoke<TResponse = unknown>(
    command: string,
    input: unknown,
    options?: InvokeOptions,
  ): Promise<TResponse> {
    const timeout = options?.timeoutMs ?? this.defaultTimeout
    const startTime = Date.now()

    // Step 1: Create command
    const createResponse = await this.createCommand(command, input, options)

    // Step 2: Poll for completion
    const pollInterval = options?.pollIntervalMs ?? 500
    const maxPollInterval = options?.maxPollIntervalMs ?? 5000
    const pollBackoff = options?.pollBackoff ?? 1.5
    let currentInterval = pollInterval

    while (Date.now() - startTime < timeout) {
      await this.sleep(currentInterval)

      const status = await this.getCommandStatus(createResponse.commandId)

      if (isTerminalState(status.state)) {
        return await this.handleTerminalState(command, status)
      }

      // Exponential backoff
      currentInterval = Math.min(currentInterval * pollBackoff, maxPollInterval)
    }

    // Timeout
    const finalStatus = await this.getCommandStatus(createResponse.commandId)
    throw new AlienError(
      CommandTimeoutError.create({
        commandId: createResponse.commandId,
        command,
        timeoutMs: timeout,
        lastState: finalStatus.state,
      }),
    )
  }

  /**
   * Fetch `path` on the manager, raise {@link ManagerHttpError} on a non-2xx
   * status, and validate the 2xx JSON body against `schema` (a malformed body
   * raises {@link MalformedResponseError}). AlienErrors — both of those — pass
   * through untouched; any other (network/transport) error is wrapped with the
   * caller-supplied context. Only 2xx responses carry a body, so `schema` is
   * always applied.
   */
  /**
   * Build a request URL by appending `path` to the configured manager base URL.
   *
   * Uses URL-based construction rather than a naive `base + path` concat so a
   * base URL carrying a query string — e.g. `https://h/v1?token=x` — keeps its
   * query correctly at the end (`https://h/v1/commands?token=x`) instead of
   * corrupting it into `https://h/v1?token=x/commands`. Twin of the receiver's
   * `buildLeaseEndpoint`, which appends path segments while leaving the query
   * untouched.
   */
  private buildManagerUrl(path: string): string {
    const url = new URL(this.managerUrl)
    const basePath = url.pathname.replace(/\/+$/, "")
    const suffix = path.startsWith("/") ? path : `/${path}`
    url.pathname = `${basePath}${suffix}`
    return url.toString()
  }

  private async managerFetch<T>(
    method: string,
    path: string,
    schema: WireSchema<T>,
    options: {
      body?: unknown
      describeError: (reason: string) => Parameters<AlienError["withContext"]>[0]
    },
  ): Promise<T> {
    const url = this.buildManagerUrl(path)
    try {
      const headers: Record<string, string> = { Authorization: `Bearer ${this.token}` }
      const init: RequestInit = { method, headers }
      if (options.body !== undefined) {
        headers["Content-Type"] = "application/json"
        init.body = JSON.stringify(options.body)
      }

      const response = await this.fetchImpl(url, init)
      if (!response.ok) {
        const errorBody = await response.text().catch(() => "")
        throw new AlienError(
          ManagerHttpError.create({
            method,
            url,
            status: response.status,
            statusText: response.statusText,
            body: errorBody,
          }),
        )
      }

      return parseWireResponse(schema, await response.json(), method, url)
    } catch (error) {
      if (error instanceof AlienError) {
        throw error
      }
      const alienError = await AlienError.from(error)
      throw alienError.withContext(
        options.describeError(error instanceof Error ? error.message : String(error)),
      )
    }
  }

  /**
   * Create a command.
   */
  private createCommand(
    command: string,
    input: unknown,
    options?: InvokeOptions,
  ): Promise<CreateCommandResponse> {
    return this.managerFetch("POST", "/v1/commands", CreateCommandResponseSchema, {
      body: {
        deploymentId: this.deploymentId,
        command,
        params: createBodySpec(input),
        deadline: options?.deadline?.toISOString(),
        idempotencyKey: options?.idempotencyKey,
        targetResourceId: options?.targetResourceId,
      },
      describeError: reason =>
        CommandCreationFailedError.create({
          deploymentId: this.deploymentId,
          command,
          reason,
        }),
    })
  }

  /**
   * Get a command's status.
   */
  private getCommandStatus(commandId: string): Promise<CommandStatusResponse> {
    return this.managerFetch("GET", `/v1/commands/${commandId}`, CommandStatusResponseSchema, {
      describeError: reason => CommandStatusFailedError.create({ commandId, reason }),
    })
  }

  /**
   * Handle a terminal state and return the response.
   */
  private async handleTerminalState<TResponse>(
    command: string,
    status: CommandStatusResponse,
  ): Promise<TResponse> {
    if (status.state === "EXPIRED") {
      throw new AlienError(
        CommandExpiredError.create({
          commandId: status.commandId,
          command,
        }),
      )
    }

    if (!status.response) {
      throw new AlienError(
        ResponseDecodingFailedError.create({
          commandId: status.commandId,
          command,
          reason: "Terminal state but no response present",
        }),
      )
    }

    const response: CommandResponse = status.response

    if (response.status === "error") {
      throw new AlienError(
        DeploymentCommandError.create({
          commandId: status.commandId,
          command,
          errorCode: response.code,
          errorMessage: response.message,
          errorDetails: response.details ?? undefined,
        }),
      )
    }

    // Decode success response
    try {
      return (await decodeBodySpec(
        response.response,
        status.commandId,
        command,
        this.allowLocalStorage,
      )) as TResponse
    } catch (error) {
      if (error instanceof AlienError) {
        throw error
      }

      throw new AlienError(
        ResponseDecodingFailedError.create({
          commandId: status.commandId,
          command,
          reason: error instanceof Error ? error.message : String(error),
        }),
      )
    }
  }

  /**
   * Sleep helper.
   */
  private sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms))
  }
}

/**
 * A {@link CommandsClient} scoped to one target command-capable resource.
 *
 * Obtained via {@link CommandsClient.target}. Every `invoke` presets
 * `targetResourceId` to this builder's resource id, overriding any
 * `options.targetResourceId` the caller passes (builder wins — same rule as the
 * Rust `TargetedCommands`).
 */
export class TargetedCommands {
  constructor(
    private readonly client: CommandsClient,
    private readonly resourceId: string,
  ) {}

  /**
   * Invoke a command against this builder's target and wait for the result.
   * The builder's target overrides any `options.targetResourceId`.
   */
  invoke<TResponse = unknown>(
    command: string,
    input: unknown,
    options?: InvokeOptions,
  ): Promise<TResponse> {
    return this.client.invoke<TResponse>(command, input, {
      ...options,
      targetResourceId: this.resourceId,
    })
  }
}
