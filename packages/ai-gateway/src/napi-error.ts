/**
 * Recovers the addon's error envelope. The Rust side serializes
 * `{ code, message, context?, retryable, internal }` into the napi error's message; without
 * this decoder the caller sees a raw JSON blob as `Error.message` and the envelope's
 * `retryable`/`internal` flags — the whole point of carrying them across the boundary — are
 * unreachable.
 *
 * Mirrors `unwrapNapiError` in `@alienplatform/bindings` (which does not yet carry `internal`).
 */

import { AlienError } from "@alienplatform/core"

/** Fallback code for napi-internal errors whose message is not an envelope. */
const GENERIC_GATEWAY_CODE = "AI_GATEWAY_ERROR"

/** The structured payload the addon serializes into `err.message`. */
interface NapiErrorEnvelope {
  code: string
  message: string
  context?: Record<string, unknown>
  retryable?: boolean
  internal?: boolean
  httpStatusCode?: number
  hint?: string
}

/**
 * Parse the addon error envelope out of `err.message`. `undefined` for non-JSON messages
 * (napi-internal errors such as a failed addon load) or JSON lacking a string `code`.
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
 * Recover a typed {@link AlienError} from an error thrown by the native addon, preserving
 * the envelope's `code`, `message`, `context`, and `retryable` flag. An error that is
 * already an `AlienError` passes through; a non-envelope message is wrapped generically.
 */
export function unwrapNapiError(err: unknown): AlienError {
  if (err instanceof AlienError) {
    return err
  }

  const rawMessage = err instanceof Error ? err.message : String(err)
  const envelope = parseEnvelope(rawMessage)

  if (!envelope) {
    // A non-envelope message is a napi-internal failure (panic, addon load) whose
    // raw text may carry sensitive detail — fail closed on redaction.
    return new AlienError({
      code: GENERIC_GATEWAY_CODE,
      message: rawMessage,
      retryable: false,
      internal: true,
    })
  }

  return new AlienError({
    code: envelope.code,
    message: envelope.message ?? rawMessage,
    retryable: envelope.retryable ?? false,
    // Honor the Rust error's redaction posture; default closed if a pre-`internal`
    // addon omits it.
    internal: envelope.internal ?? true,
    context: envelope.context ?? {},
    // Carry the Rust error's status code through rather than defaulting to 500,
    // so a startup config error (e.g. 400) is not rendered as a server fault.
    httpStatusCode: envelope.httpStatusCode,
    hint: envelope.hint,
  })
}
