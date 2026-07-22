/**
 * Typed errors for the AI client surface (`ai()` / `getAiConnection`).
 *
 * Defined here rather than in `@alienplatform/bindings` because the AI binding
 * resolves through this package's gateway process, not the bindings native addon.
 */

import { defineError } from "@alienplatform/core"
import * as z from "zod/v4"

// Shared with the binding surfaces in @alienplatform/bindings.
export { BindingNotFoundError, InvalidBindingConfigError } from "@alienplatform/core"

/**
 * Error thrown when an upstream LLM endpoint returns a non-2xx response.
 *
 * Marked internal because the response body and upstream URL may contain
 * provider-specific infra detail not intended for external clients.
 * Retryability is derived per-call from the HTTP status code.
 */
export const AiUpstreamError = defineError({
  code: "AI_UPSTREAM_ERROR",
  context: z.object({
    url: z.string(),
    status: z.number(),
    message: z.string(),
  }),
  message: ({ url, status, message }) =>
    `AI upstream request to '${url}' failed with status ${status}: ${message}`,
  retryable: false,
  internal: true,
  httpStatusCode: 502,
})

/**
 * Error thrown when a network-level failure occurs before or after an
 * upstream LLM request (fetch threw, or the success body was not parseable JSON).
 * These are always transient and safe to retry.
 */
export const AiTransportError = defineError({
  code: "AI_TRANSPORT_ERROR",
  context: z.object({
    url: z.string(),
    reason: z.string(),
  }),
  message: ({ url, reason }) => `AI request to '${url}' failed due to a transport error: ${reason}`,
  retryable: true,
  internal: true,
  httpStatusCode: 503,
})

/**
 * Error thrown when this platform/architecture has no prebuilt gateway binary.
 */
export const UnsupportedPlatformError = defineError({
  code: "AI_GATEWAY_UNSUPPORTED_PLATFORM",
  context: z.object({
    platform: z.string(),
    arch: z.string(),
    reason: z.string().optional(),
  }),
  message: ({ platform, arch, reason }) =>
    `@alienplatform/ai-gateway has no prebuilt binary for platform '${platform}' arch '${arch}'${reason ? `: ${reason}` : ""}.`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Error thrown when the `alien-ai-gateway` executable cannot be located (or, for a
 * compiled Worker, extracted) for this platform. Internal because the context
 * carries filesystem paths.
 */
export const GatewayBinaryUnavailableError = defineError({
  code: "AI_GATEWAY_BINARY_UNAVAILABLE",
  context: z.object({
    triple: z.string(),
    reason: z.string(),
    path: z.string().optional(),
  }),
  message: ({ triple, reason }) =>
    `Cannot locate the alien-ai-gateway binary for '${triple}': ${reason}`,
  retryable: false,
  internal: true,
  // Same class as UnsupportedPlatformError: the host environment can't run the binary.
  httpStatusCode: 400,
})

/**
 * Error thrown when the spawned `alien-ai-gateway` process failed to report a
 * ready URL: it exited early or errored on startup. Retryable because the common
 * cause (an ambient cloud credential not yet resolvable) is transient.
 */
export const GatewayStartFailedError = defineError({
  code: "AI_GATEWAY_START_FAILED",
  context: z.object({
    reason: z.string(),
  }),
  message: ({ reason }) => `The alien-ai-gateway process failed to start: ${reason}`,
  retryable: true,
  internal: true,
  httpStatusCode: 503,
})
