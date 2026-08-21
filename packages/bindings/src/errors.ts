/**
 * Error definitions and napi error recovery for `@alienplatform/bindings`.
 *
 * The native addon runs every binding operation as an async napi method. napi
 * constrains async errors to a fixed `Status` string, so `err.code` is always
 * `"GenericFailure"` and carries no information. The addon instead serializes a
 * structured envelope into the JS `err.message`:
 *
 *   { code, message, context?, retryable, internal, httpStatusCode?, hint? }
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

/**
 * Thrown when the native addon reports a Postgres `sslmode` this package does not
 * recognize, which means the addon and this wrapper disagree about the Rust `SslMode`
 * enum — a version skew between the two halves of the package.
 *
 * `sslmode` is the value the addon sent; `expected` is the set this wrapper accepts.
 * Not retryable: a skew is fixed by aligning versions, not by trying again.
 */
export const UnknownPostgresSslModeError = defineError({
  code: "UNKNOWN_POSTGRES_SSLMODE",
  context: z.object({
    sslmode: z.string(),
    expected: z.array(z.string()),
  }),
  message: ({ sslmode, expected }) =>
    `@alienplatform/bindings received an unknown Postgres sslmode '${sslmode}' from the native addon; expected one of ${expected.join(", ")}.`,
  retryable: false,
  internal: false,
})

/**
 * Thrown when the native addon reports a Postgres TLS policy that cannot be
 * applied safely, such as a verified mode without any CA roots.
 *
 * This indicates wrapper/addon version skew or a malformed resolved binding.
 * It is not retryable because retrying cannot repair the contract.
 */
export const InvalidPostgresTlsConfigError = defineError({
  code: "INVALID_POSTGRES_TLS_CONFIG",
  context: z.object({
    sslmode: z.string(),
    reason: z.string(),
  }),
  message: ({ sslmode, reason }) =>
    `@alienplatform/bindings received an invalid Postgres TLS configuration for sslmode '${sslmode}' from the native addon: ${reason}.`,
  retryable: false,
  internal: false,
})

/**
 * Thrown when the native addon reports a sandbox session state or output frame kind this wrapper
 * does not know.
 *
 * Casting instead would put a value outside the declared union behind a type that says otherwise,
 * so a `switch` a caller wrote against the union would fall through silently. Version skew has to
 * fail loudly, and retrying cannot repair it.
 */
export const UnknownSandboxValueError = defineError({
  code: "UNKNOWN_SANDBOX_VALUE",
  context: z.object({
    field: z.string(),
    value: z.string(),
    expected: z.array(z.string()),
  }),
  message: ({ field, value, expected }) =>
    `@alienplatform/bindings received an unknown sandbox ${field} '${value}' from the native addon; expected one of ${expected.join(", ")}.`,
  retryable: false,
  internal: false,
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
  internal?: boolean
  httpStatusCode?: number
  hint?: string | null
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
 *   `retryable`, `internal`, `httpStatusCode`, and `hint` metadata.
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
    internal: envelope.internal ?? false,
    httpStatusCode: envelope.httpStatusCode,
    hint: envelope.hint,
    context,
  })
}

// Shared with the AI binding surface in @alienplatform/ai-gateway.
export { BindingNotFoundError } from "@alienplatform/core"
