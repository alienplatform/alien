/**
 * Command sender — invoke commands on remote Alien deployments.
 *
 * Migrated from `@alienplatform/sdk/commands`. Pure `fetch`; no gRPC, no
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

import { AlienError } from "@alienplatform/core"
import {
  CommandCreationFailedError,
  CommandExpiredError,
  CommandTimeoutError,
  DeploymentCommandError,
  ManagerHttpError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"
import { downloadPresigned } from "./presigned.js"
import type {
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CreateCommandResponse,
} from "./protocol.js"

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
 * Serialize `data` (JSON-stringifying anything that isn't already a string) and
 * base64-encode it. This is the one place the send-side serialize decision
 * lives. `@alienplatform/commands` is a Node-only package — the receiver decodes
 * with `Buffer` too — so this uses `Buffer` directly rather than branching on a
 * browser `btoa`.
 */
function base64Encode(data: unknown): string {
  const json = typeof data === "string" ? data : JSON.stringify(data)
  return Buffer.from(json, "utf-8").toString("base64")
}

/**
 * Base64 decode data.
 */
function base64Decode(encoded: string): string {
  if (typeof atob !== "undefined") {
    // Browser
    return atob(encoded)
  }
  // Node.js
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
      request.backend.type === "http" ? request.backend.url : `local://${request.backend.filePath}`

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

  constructor(config: CommandsClientConfig) {
    this.managerUrl = config.managerUrl.replace(/\/+$/, "")
    this.deploymentId = config.deploymentId
    this.token = config.token
    this.defaultTimeout = config.timeoutMs ?? 60_000
    this.allowLocalStorage = config.allowLocalStorage ?? false
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
   * Create a command.
   */
  private async createCommand(
    command: string,
    input: unknown,
    options?: InvokeOptions,
  ): Promise<CreateCommandResponse> {
    const url = `${this.managerUrl}/v1/commands`
    const body = {
      deploymentId: this.deploymentId,
      command,
      params: createBodySpec(input),
      deadline: options?.deadline?.toISOString(),
      idempotencyKey: options?.idempotencyKey,
      targetResourceId: options?.targetResourceId,
    }

    try {
      const response = await fetch(url, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.token}`,
        },
        body: JSON.stringify(body),
      })

      if (!response.ok) {
        const errorBody = await response.text().catch(() => "")
        throw new AlienError(
          ManagerHttpError.create({
            method: "POST",
            url,
            status: response.status,
            statusText: response.statusText,
            body: errorBody,
          }),
        )
      }

      return (await response.json()) as CreateCommandResponse
    } catch (error) {
      if (error instanceof AlienError) {
        throw error
      }

      const alienError = await AlienError.from(error)
      throw alienError.withContext(
        CommandCreationFailedError.create({
          deploymentId: this.deploymentId,
          command,
          reason: error instanceof Error ? error.message : String(error),
        }),
      )
    }
  }

  /**
   * Get a command's status.
   */
  private async getCommandStatus(commandId: string): Promise<CommandStatusResponse> {
    const url = `${this.managerUrl}/v1/commands/${commandId}`

    try {
      const response = await fetch(url, {
        method: "GET",
        headers: {
          Authorization: `Bearer ${this.token}`,
        },
      })

      if (!response.ok) {
        const errorBody = await response.text().catch(() => "")
        throw new AlienError(
          ManagerHttpError.create({
            method: "GET",
            url,
            status: response.status,
            statusText: response.statusText,
            body: errorBody,
          }),
        )
      }

      return (await response.json()) as CommandStatusResponse
    } catch (error) {
      if (error instanceof AlienError) {
        throw error
      }

      throw await AlienError.from(error)
    }
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
