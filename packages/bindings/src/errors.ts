/**
 * Error definitions and napi error recovery for `@alienplatform/bindings`.
 *
 * The native addon runs every binding operation as an async napi method. napi
 * constrains async errors to a fixed `Status` string, so `err.code` is always
 * `"GenericFailure"` and carries no information. The addon instead serializes a
 * structured envelope into the JS `err.message`:
 *
 *   { code, message, context?, retryable }
 *
 * where `context` keys are snake_case (e.g. `binding_name`, `env_var`).
 * `unwrapNapiError` recovers that envelope with a single `JSON.parse` — never by
 * scraping the human message — and maps it to a typed {@link AlienError}.
 */

import { AlienError, defineError } from "@alienplatform/core"
import * as z from "zod/v4"

// Re-exported so consumers handle bindings errors without importing
// `@alienplatform/core` directly.
export { AlienError, defineError }

/**
 * Thrown on the first operation against a binding that has no
 * `ALIEN_<NAME>_BINDING` entry in the environment.
 *
 * `binding` is the binding name; `envVar` is the missing `ALIEN_<NAME>_BINDING`
 * variable. Both are mapped from the addon envelope's snake_case
 * `binding_name` / `env_var` context keys.
 */
export const BindingNotConfiguredError = defineError({
  code: "BINDING_NOT_CONFIGURED",
  context: z.object({
    binding: z.string(),
    envVar: z.string(),
  }),
  message: ({ binding, envVar }) =>
    `Binding '${binding}' is not configured. Set the '${envVar}' environment variable.`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/** Fallback code for napi-internal errors whose message is not an envelope. */
const GENERIC_BINDINGS_CODE = "BINDINGS_ERROR"

/** Envelope codes the wrapper maps to a dedicated typed error. */
const BINDING_NOT_CONFIGURED = "BINDING_NOT_CONFIGURED"

/** The structured payload the addon serializes into `err.message`. */
interface NapiErrorEnvelope {
  code: string
  message: string
  context?: Record<string, unknown>
  retryable?: boolean
}

/**
 * Attempt to parse the addon error envelope out of `err.message`.
 *
 * Returns `undefined` for non-JSON messages (napi-internal errors such as a
 * failed addon load, which are not envelopes) or JSON that lacks a string
 * `code`.
 */
function parseEnvelope(rawMessage: string): NapiErrorEnvelope | undefined {
  let parsed: unknown
  try {
    parsed = JSON.parse(rawMessage)
  } catch {
    return undefined
  }
  if (
    parsed !== null &&
    typeof parsed === "object" &&
    typeof (parsed as { code?: unknown }).code === "string"
  ) {
    return parsed as NapiErrorEnvelope
  }
  return undefined
}

/**
 * Recover a typed {@link AlienError} from an error thrown by the native addon.
 *
 * - An error that is already an {@link AlienError} passes through unchanged.
 * - A message carrying the addon envelope is decoded: `BINDING_NOT_CONFIGURED`
 *   becomes {@link BindingNotConfiguredError} (with `binding` / `envVar` mapped
 *   from the envelope's snake_case context); every other envelope code becomes a
 *   generic `AlienError` that preserves the `code`, `message`, `context`, and
 *   `retryable` flag.
 * - A non-envelope message (napi-internal error) is wrapped as a generic
 *   `BINDINGS_ERROR`, preserving the original message.
 */
export function unwrapNapiError(err: unknown): AlienError {
  if (err instanceof AlienError) {
    return err
  }

  const rawMessage = err instanceof Error ? err.message : String(err)
  const envelope = parseEnvelope(rawMessage)

  if (!envelope) {
    return new AlienError({
      code: GENERIC_BINDINGS_CODE,
      message: rawMessage,
      retryable: false,
      internal: false,
    })
  }

  const context = envelope.context ?? {}

  if (envelope.code === BINDING_NOT_CONFIGURED) {
    // `.toOptions()` yields the generic `AlienError` (default context), avoiding
    // the narrower `AlienError<ZodObject<...>>` that the definition overload
    // would produce, while still generating the typed message.
    return new AlienError(
      BindingNotConfiguredError.create({
        binding: String(context.binding_name ?? ""),
        envVar: String(context.env_var ?? ""),
      }).toOptions(),
    )
  }

  return new AlienError({
    code: envelope.code,
    message: envelope.message ?? rawMessage,
    retryable: envelope.retryable ?? false,
    internal: false,
    context,
  })
}

// Shared with the AI binding surface in @alienplatform/ai-gateway.
export { BindingNotFoundError } from "@alienplatform/core"
