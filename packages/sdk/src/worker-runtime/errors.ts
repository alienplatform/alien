/**
 * Errors internal to the Worker runtime.
 *
 * These are the gRPC/transport and environment errors the Worker runtime raises
 * while connecting to the runtime and dispatching tasks. They are confined to
 * the `./worker-runtime` subpath — the facade root does not re-export them (the
 * app-facing error surface is `BindingNotConfiguredError`/`AlienError` from
 * `@alienplatform/bindings` and `@alienplatform/core`).
 */

import { defineError } from "@alienplatform/core"
import * as z from "zod/v4"

/**
 * Error thrown when the gRPC connection to the runtime fails.
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
 * Error thrown when the Worker runtime configuration is invalid.
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
