/**
 * Typed errors for the AI client surface (`ai()` / `getAiConnection`).
 *
 * Defined here rather than in `@alienplatform/bindings` because the AI binding
 * resolves through this package's gateway, not the bindings native addon.
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
 * Error thrown when this platform/architecture has no native addon.
 */
export const UnsupportedPlatformError = defineError({
  code: "AI_GATEWAY_UNSUPPORTED_PLATFORM",
  context: z.object({
    platform: z.string(),
    arch: z.string(),
    reason: z.string().optional(),
  }),
  message: ({ platform, arch, reason }) =>
    `@alienplatform/ai-gateway has no native addon for platform '${platform}' arch '${arch}'${reason ? `: ${reason}` : ""}.`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Error thrown when the native addon exists but could not be loaded. Internal because the
 * context carries filesystem paths.
 */
export const NativeAddonLoadFailedError = defineError({
  code: "AI_GATEWAY_ADDON_LOAD_FAILED",
  context: z.object({
    triple: z.string(),
    reason: z.string(),
    path: z.string().optional(),
  }),
  message: ({ triple, reason }) =>
    `Cannot load the @alienplatform/ai-gateway native addon for '${triple}': ${reason}`,
  retryable: false,
  internal: true,
  // Same class as UnsupportedPlatformError: the host environment can't run the addon.
  httpStatusCode: 400,
})
