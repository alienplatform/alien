/**
 * Error definitions for the Alien bindings SDK.
 * These errors are thrown when binding operations fail.
 */

import { defineError } from "@aliendotdev/core"
import * as z from "zod/v4"

/**
 * Error thrown when the gRPC connection to the alien-runtime fails.
 */
export const GrpcConnectionError = defineError({
  code: "GRPC_CONNECTION_ERROR",
  context: z.object({
    endpoint: z.string(),
    reason: z.string(),
  }),
  message: ({ endpoint, reason }) =>
    `Failed to connect to alien-runtime at '${endpoint}': ${reason}`,
  retryable: true,
  internal: false,
  httpStatusCode: 503,
})

/**
 * Error thrown when a gRPC call fails.
 */
export const GrpcCallError = defineError({
  code: "GRPC_CALL_ERROR",
  context: z.object({
    service: z.string(),
    method: z.string(),
    grpcCode: z.string(),
    details: z.string(),
  }),
  message: ({ service, method, grpcCode, details }) =>
    `gRPC call ${service}.${method} failed with code ${grpcCode}: ${details}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when a binding is not found.
 */
export const BindingNotFoundError = defineError({
  code: "BINDING_NOT_FOUND",
  context: z.object({
    bindingName: z.string(),
    bindingType: z.string(),
  }),
  message: ({ bindingName, bindingType }) => `${bindingType} binding '${bindingName}' not found`,
  retryable: false,
  internal: false,
  httpStatusCode: 404,
})

/**
 * Error thrown when the AlienContext is not initialized.
 */
export const ContextNotInitializedError = defineError({
  code: "CONTEXT_NOT_INITIALIZED",
  context: z.object({
    operation: z.string(),
  }),
  message: ({ operation }) =>
    `AlienContext not initialized. Call AlienContext.fromEnv() before using ${operation}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when a required environment variable is missing.
 */
export const MissingEnvVarError = defineError({
  code: "MISSING_ENV_VAR",
  context: z.object({
    variable: z.string(),
    description: z.string(),
  }),
  message: ({ variable, description }) =>
    `Required environment variable '${variable}' is not set. ${description}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when a storage object is not found.
 */
export const StorageObjectNotFoundError = defineError({
  code: "STORAGE_OBJECT_NOT_FOUND",
  context: z.object({
    bindingName: z.string(),
    path: z.string(),
  }),
  message: ({ bindingName, path }) =>
    `Object '${path}' not found in storage binding '${bindingName}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 404,
})

/**
 * Error thrown when a storage precondition fails.
 */
export const StoragePreconditionError = defineError({
  code: "STORAGE_PRECONDITION_FAILED",
  context: z.object({
    bindingName: z.string(),
    path: z.string(),
    condition: z.string(),
  }),
  message: ({ bindingName, path, condition }) =>
    `Precondition failed for '${path}' in storage binding '${bindingName}': ${condition}`,
  retryable: false,
  internal: false,
  httpStatusCode: 412,
})

/**
 * Error thrown when a storage object already exists (for create mode).
 */
export const StorageObjectExistsError = defineError({
  code: "STORAGE_OBJECT_EXISTS",
  context: z.object({
    bindingName: z.string(),
    path: z.string(),
  }),
  message: ({ bindingName, path }) =>
    `Object '${path}' already exists in storage binding '${bindingName}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 409,
})

/**
 * Error thrown when a KV key is not found.
 */
export const KvKeyNotFoundError = defineError({
  code: "KV_KEY_NOT_FOUND",
  context: z.object({
    bindingName: z.string(),
    key: z.string(),
  }),
  message: ({ bindingName, key }) => `Key '${key}' not found in KV binding '${bindingName}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 404,
})

/**
 * Error thrown when a KV key is invalid.
 */
export const KvInvalidKeyError = defineError({
  code: "KV_INVALID_KEY",
  context: z.object({
    key: z.string(),
    reason: z.string(),
  }),
  message: ({ key, reason }) => `Invalid KV key '${key}': ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Error thrown when a KV value is invalid.
 */
export const KvInvalidValueError = defineError({
  code: "KV_INVALID_VALUE",
  context: z.object({
    reason: z.string(),
  }),
  message: ({ reason }) => `Invalid KV value: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Error thrown when a secret is not found.
 */
export const SecretNotFoundError = defineError({
  code: "SECRET_NOT_FOUND",
  context: z.object({
    bindingName: z.string(),
    secretName: z.string(),
  }),
  message: ({ bindingName, secretName }) =>
    `Secret '${secretName}' not found in vault binding '${bindingName}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 404,
})

/**
 * Error thrown when a queue operation fails.
 */
export const QueueOperationError = defineError({
  code: "QUEUE_OPERATION_ERROR",
  context: z.object({
    bindingName: z.string(),
    queue: z.string(),
    operation: z.string(),
    reason: z.string(),
  }),
  message: ({ bindingName, queue, operation, reason }) =>
    `Queue ${operation} failed for queue '${queue}' in binding '${bindingName}': ${reason}`,
  retryable: true,
  internal: false,
  httpStatusCode: 500,
})

/**
 * Error thrown when an event handler is already registered.
 */
export const EventHandlerAlreadyRegisteredError = defineError({
  code: "EVENT_HANDLER_ALREADY_REGISTERED",
  context: z.object({
    handlerType: z.string(),
    resourceName: z.string(),
  }),
  message: ({ handlerType, resourceName }) =>
    `Event handler for ${handlerType} '${resourceName}' is already registered`,
  retryable: false,
  internal: false,
  httpStatusCode: 409,
})

/**
 * Error thrown when a command is already registered.
 */
export const CommandAlreadyRegisteredError = defineError({
  code: "COMMAND_ALREADY_REGISTERED",
  context: z.object({
    commandName: z.string(),
  }),
  message: ({ commandName }) => `Command '${commandName}' is already registered`,
  retryable: false,
  internal: false,
  httpStatusCode: 409,
})

/**
 * Error thrown when the binding configuration is invalid.
 */
export const InvalidBindingConfigError = defineError({
  code: "INVALID_BINDING_CONFIG",
  context: z.object({
    message: z.string(),
    suggestion: z.string().optional(),
  }),
  message: ({ message, suggestion }) => (suggestion ? `${message}. ${suggestion}` : message),
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})
