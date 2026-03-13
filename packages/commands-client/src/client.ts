/**
 * Commands Client - Lightweight client for deployment command invocation
 * 
 * Handles command invocation with automatic:
 * - Base64 encoding/decoding of params and responses
 * - Large payload upload/download via storage
 * - Polling with exponential backoff
 * - Error handling and timeout
 */

import { AlienError } from "@aliendotdev/core"
import type {
  CommandsClientConfig,
  BodySpec,
  CommandResponse,
  CommandState,
  CommandStatusResponse,
  CreateCommandResponse,
  InvokeOptions,
} from "./types.js"
import {
  DeploymentCommandError,
  ManagerHttpError,
  CommandCreationFailedError,
  CommandExpiredError,
  CommandTimeoutError,
  ResponseDecodingFailedError,
  StorageOperationFailedError,
} from "./errors.js"

const INLINE_MAX_BYTES = 150_000 // 150KB - matches server limit

/**
 * Base64 encode data
 */
function base64Encode(data: string | Record<string, unknown>): string {
  const json = typeof data === "string" ? data : JSON.stringify(data)
  if (typeof btoa !== "undefined") {
    // Browser
    return btoa(json)
  }
  // Node.js
  return Buffer.from(json, "utf-8").toString("base64")
}

/**
 * Base64 decode data
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
 * Create inline or storage body spec based on size
 */
function createBodySpec(data: string | Record<string, unknown>): BodySpec {
  const json = typeof data === "string" ? data : JSON.stringify(data)
  const bytes = new TextEncoder().encode(json).length

  if (bytes <= INLINE_MAX_BYTES) {
    return {
      mode: "inline",
      inlineBase64: base64Encode(json),
    }
  }

  return {
    mode: "storage",
    size: bytes,
  }
}

/**
 * Decode response body spec
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

    // Check if request has expired
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

    // Handle different backend types
    if (request.backend.type === "http") {
      // HTTP backend (AWS S3, GCP GCS, Azure Blob)
      try {
        const response = await fetch(request.backend.url, {
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

        const text = await response.text()
        return JSON.parse(text)
      } catch (error) {
        if (error instanceof AlienError) {
          throw error
        }

        // Wrap fetch/parse errors
        const alienError = await AlienError.from(error)
        throw alienError.withContext(
          StorageOperationFailedError.create({
            operation: "download",
            url: request.backend.url,
            reason: error instanceof Error ? error.message : String(error),
          }),
        )
      }
    } else if (request.backend.type === "local") {
      // Local backend: Only enabled when explicitly allowed (local dev/testing)
      if (!allowLocalStorage) {
        throw new AlienError(
          StorageOperationFailedError.create({
            operation: "download",
            url: `local://${request.backend.filePath}`,
            reason: "Local storage backend not enabled (set allowLocalStorage: true for local dev)",
          }),
        )
      }
      
      // Validate path doesn't contain path traversal attempts
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
      
      // Dynamically import fs/promises only when needed (local dev)
      try {
        const { readFile } = await import("node:fs/promises")
        const content = await readFile(filePath, "utf-8")
        return JSON.parse(content)
      } catch (error) {
        if (error instanceof AlienError) {
          throw error
        }
        
        // Wrap filesystem/parse errors
        const alienError = await AlienError.from(error)
        throw alienError.withContext(
          StorageOperationFailedError.create({
            operation: "download",
            url: `local://${filePath}`,
            reason: error instanceof Error ? error.message : String(error),
          }),
        )
      }
    } else {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "download",
          url: "unknown",
          reason: `Unknown storage backend type: ${(request.backend as any).type}`,
        }),
      )
    }
  }

  throw new AlienError(
    ResponseDecodingFailedError.create({
      commandId,
      command,
      reason: `Unknown body mode: ${(body as any).mode}`,
    }),
  )
}

/**
 * Check if state is terminal
 */
function isTerminalState(state: CommandState): boolean {
  return state === "SUCCEEDED" || state === "FAILED" || state === "EXPIRED"
}

/**
 * Commands client for invoking deployment commands
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
    this.defaultTimeout = config.timeout ?? 60_000
    this.allowLocalStorage = config.allowLocalStorage ?? false
  }

  /**
   * Invoke a command on the deployment and wait for response
   * 
   * @param command - Command name (e.g., "generate-report")
   * @param params - Command parameters (JSON-serializable)
   * @param options - Invocation options
   * @returns Decoded response data
   */
  async invoke<TParams = unknown, TResponse = unknown>(
    command: string,
    params: TParams,
    options?: InvokeOptions,
  ): Promise<TResponse> {
    const timeout = options?.timeout ?? this.defaultTimeout
    const startTime = Date.now()

    // Step 1: Create command
    const createResponse = await this.createCommand(command, params, options)

    // Step 2: Handle storage upload if needed
    if (createResponse.next === "upload") {
      const uploadUrl =
        createResponse.storageUpload?.putRequest.backend.type === "http"
          ? createResponse.storageUpload.putRequest.backend.url
          : "unknown"
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: uploadUrl,
          reason: "Large payload uploads not yet supported",
        }),
      )
    }

    // Step 3: Poll for completion
    const pollInterval = options?.pollInterval ?? 500
    const maxPollInterval = options?.maxPollInterval ?? 5000
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
   * Create a command
   */
  private async createCommand(
    command: string,
    params: unknown,
    options?: InvokeOptions,
  ): Promise<CreateCommandResponse> {
    const url = `${this.managerUrl}/v1/commands`
    const body = {
      deploymentId: this.deploymentId,
      command,
      params: createBodySpec(params as string | Record<string, unknown>),
      deadline: options?.deadline?.toISOString(),
      idempotencyKey: options?.idempotencyKey,
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
   * Get command status
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
   * Handle terminal state and return response
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
   * Sleep helper
   */
  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms))
  }
}
