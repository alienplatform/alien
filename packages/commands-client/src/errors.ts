/**
 * Commands Client Error Definitions
 */

import { defineError } from "@aliendotdev/core"
import { z } from "zod/v4"

/**
 * Error thrown when command creation fails.
 */
export const CommandCreationFailedError = defineError({
  code: "COMMAND_CREATION_FAILED",
  context: z.object({
    deploymentId: z.string(),
    command: z.string(),
    reason: z.string(),
  }),
  message: ({ deploymentId, command, reason }) =>
    `Failed to create command '${command}' for deployment '${deploymentId}': ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when command execution times out.
 */
export const CommandTimeoutError = defineError({
  code: "COMMAND_TIMEOUT",
  context: z.object({
    commandId: z.string(),
    command: z.string(),
    timeoutMs: z.number(),
    lastState: z.string(),
  }),
  message: ({ command, timeoutMs, lastState }) =>
    `Command '${command}' timed out after ${timeoutMs}ms (last state: ${lastState})`,
  retryable: true,
  internal: false,
  httpStatusCode: 504,
})

/**
 * Error thrown when the deployment returns an error response.
 */
export const DeploymentCommandError = defineError({
  code: "DEPLOYMENT_COMMAND_ERROR",
  context: z.object({
    commandId: z.string(),
    command: z.string(),
    errorCode: z.string(),
    errorMessage: z.string(),
    errorDetails: z.string().optional(),
  }),
  message: ({ command, errorCode, errorMessage }) =>
    `Deployment command '${command}' failed: [${errorCode}] ${errorMessage}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when command expires before completion.
 */
export const CommandExpiredError = defineError({
  code: "COMMAND_EXPIRED",
  context: z.object({
    commandId: z.string(),
    command: z.string(),
  }),
  message: ({ command }) => `Command '${command}' expired before completion`,
  retryable: false,
  internal: false,
  httpStatusCode: 410,
})

/**
 * Error thrown when storage upload/download fails.
 */
export const StorageOperationFailedError = defineError({
  code: "STORAGE_OPERATION_FAILED",
  context: z.object({
    operation: z.enum(["upload", "download"]),
    url: z.string(),
    reason: z.string(),
  }),
  message: ({ operation, reason }) => `Storage ${operation} failed: ${reason}`,
  retryable: true,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when response decoding fails.
 */
export const ResponseDecodingFailedError = defineError({
  code: "RESPONSE_DECODING_FAILED",
  context: z.object({
    commandId: z.string(),
    command: z.string(),
    reason: z.string(),
  }),
  message: ({ command, reason }) => `Failed to decode response for command '${command}': ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when Manager returns an HTTP error.
 */
export const ManagerHttpError = defineError({
  code: "MANAGER_HTTP_ERROR",
  context: z.object({
    method: z.string(),
    url: z.string(),
    status: z.number(),
    statusText: z.string(),
    body: z.string().optional(),
  }),
  message: ({ method, url, status, statusText }) =>
    `Manager ${method} ${url} failed: ${status} ${statusText}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})
