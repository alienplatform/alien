/**
 * Command sender error definitions.
 *
 * Migrated from the former `@alienplatform/sdk/commands` subpath — the seven `defineError`
 * definitions the sender raises. Each is built with `defineError` from
 * `@alienplatform/core` so it carries the shared `AlienError` identity.
 */

import { defineError } from "@alienplatform/core"
import * as z from "zod/v4"

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
 * Error thrown when fetching a command's status fails (network/transport error,
 * not an HTTP error status — those raise {@link ManagerHttpError}).
 */
export const CommandStatusFailedError = defineError({
  code: "COMMAND_STATUS_FAILED",
  context: z.object({
    commandId: z.string(),
    reason: z.string(),
  }),
  message: ({ commandId, reason }) =>
    `Failed to fetch status for command '${commandId}': ${reason}`,
  retryable: true,
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
 * Error thrown when a command envelope (or a param/response body it carries)
 * fails to decode or validate — malformed inline base64, a storage-mode body
 * missing its presigned request, or an envelope that fails schema validation.
 *
 * The Rust twin (`alien_commands::error::ErrorData::InvalidEnvelope`) raises
 * the identical code (`INVALID_ENVELOPE`) for the same failures, so envelope
 * decode failures are twin-pinned across both receivers (see
 * `PACKAGE_LAYOUT.md` DECIDED(09)).
 */
export const InvalidEnvelopeError = defineError({
  code: "INVALID_ENVELOPE",
  context: z.object({
    field: z.string().optional(),
    reason: z.string(),
  }),
  message: ({ reason }) => `Invalid command envelope: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Error thrown when the pull receiver's environment configuration is missing or
 * invalid. Fails fast (synchronously, from `createCommandReceiver`) and names
 * the offending variable in `context.envVar`.
 *
 * The Rust twin (`alien_commands::Receiver::from_env`) raises the identical code
 * (`COMMAND_RECEIVER_CONFIG_INVALID`) for the same identity, token-source, and
 * tuning variables, so the two receivers reject the same misconfigurations.
 */
export const CommandReceiverConfigInvalidError = defineError({
  code: "COMMAND_RECEIVER_CONFIG_INVALID",
  context: z.object({
    envVar: z.string(),
    reason: z.string(),
  }),
  message: ({ reason }) => `Command receiver configuration invalid: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
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

/**
 * Error thrown when a 2xx response body from the command server fails to parse
 * or validate against its wire schema — a malformed/unexpected JSON shape
 * surfaces here as a typed error instead of a downstream `TypeError` when a
 * missing field is dereferenced.
 */
export const MalformedResponseError = defineError({
  code: "MALFORMED_RESPONSE",
  context: z.object({
    method: z.string(),
    url: z.string(),
    reason: z.string(),
  }),
  message: ({ method, url, reason }) => `Malformed response from ${method} ${url}: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 502,
})
