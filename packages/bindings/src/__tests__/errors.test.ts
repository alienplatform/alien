import { AlienError } from "@alienplatform/core"
import { describe, expect, it } from "vitest"
import { BindingNotConfiguredError, isSandboxOutcomeUnknown, unwrapNapiError } from "../errors.js"

/** Build a napi-style error whose message carries the addon envelope. */
function napiError(envelope: unknown): Error {
  return new Error(JSON.stringify(envelope))
}

describe("unwrapNapiError", () => {
  it("maps a BINDING_NOT_CONFIGURED envelope to BindingNotConfiguredError with camelCase context", () => {
    const err = unwrapNapiError(
      napiError({
        code: "BINDING_NOT_CONFIGURED",
        message: "binding not configured",
        context: { binding_name: "files", env_var: "ALIEN_FILES_BINDING" },
        retryable: false,
      }),
    )

    expect(err).toBeInstanceOf(AlienError)
    expect(err.code).toBe("BINDING_NOT_CONFIGURED")
    expect(err.code).toBe(BindingNotConfiguredError.metadata.code)
    expect(err.context).toEqual({ binding: "files", envVar: "ALIEN_FILES_BINDING" })
    expect(err.retryable).toBe(false)
  })

  it("preserves all structured metadata for other envelope codes", () => {
    const err = unwrapNapiError(
      napiError({
        code: "STORAGE_OPERATION_FAILED",
        message: "get failed",
        context: { binding_name: "files", operation: "get" },
        retryable: true,
        internal: false,
        httpStatusCode: 503,
        hint: "Retry after the customer finishes setup",
      }),
    )

    expect(err).toBeInstanceOf(AlienError)
    expect(err.code).toBe("STORAGE_OPERATION_FAILED")
    expect(err.message).toBe("get failed")
    expect(err.retryable).toBe(true)
    expect(err.internal).toBe(false)
    expect(err.httpStatusCode).toBe(503)
    expect(err.hint).toBe("Retry after the customer finishes setup")
    expect(err.context).toEqual({ binding_name: "files", operation: "get" })
  })

  it("wraps a non-JSON message as a generic BINDINGS_ERROR, preserving the message", () => {
    const err = unwrapNapiError(new Error("Failed to load native binding"))

    expect(err).toBeInstanceOf(AlienError)
    expect(err.code).toBe("BINDINGS_ERROR")
    expect(err.message).toBe("Failed to load native binding")
  })

  it("wraps JSON that lacks a string code as a generic BINDINGS_ERROR", () => {
    // Valid JSON but not an envelope (e.g. an incidental array message).
    const err = unwrapNapiError(new Error("[1,2,3]"))

    expect(err.code).toBe("BINDINGS_ERROR")
    expect(err.message).toBe("[1,2,3]")
  })

  it("passes an existing AlienError through unchanged", () => {
    const original = new AlienError(
      BindingNotConfiguredError.create({ binding: "files", envVar: "ALIEN_FILES_BINDING" }),
    )
    expect(unwrapNapiError(original)).toBe(original)
  })

  it("handles non-Error throwables by stringifying them", () => {
    const err = unwrapNapiError("boom")
    expect(err.code).toBe("BINDINGS_ERROR")
    expect(err.message).toBe("boom")
  })
})

describe("isSandboxOutcomeUnknown", () => {
  // Asserted against the envelope the addon actually emits rather than a hand-built AlienError:
  // the envelope shape is the contract, and a hand-built object cannot break when it changes.
  const decoded = (code: string) =>
    unwrapNapiError(napiError({ code, message: "m", retryable: false, internal: false }))

  it("recognizes an operation the sandbox never reported the outcome of", () => {
    expect(isSandboxOutcomeUnknown(decoded("SANDBOX_OUTCOME_UNKNOWN"))).toBe(true)
  })

  it("does not recognize a failure the sandbox answered", () => {
    expect(isSandboxOutcomeUnknown(decoded("SANDBOX_COMMAND_FAILED"))).toBe(false)
  })

  it("does not recognize a value that is not an alien error", () => {
    expect(isSandboxOutcomeUnknown(new Error("SANDBOX_OUTCOME_UNKNOWN"))).toBe(false)
  })

  // A caller that adds its own context before checking must still see the signal; reading only the
  // outermost code would answer "safe to repeat" and run the command a second time.
  it("still recognizes it after a caller has wrapped it with context", () => {
    const wrapped = decoded("SANDBOX_OUTCOME_UNKNOWN").withContext(
      BindingNotConfiguredError.create({ binding: "files", envVar: "ALIEN_FILES_BINDING" }),
    )
    expect(isSandboxOutcomeUnknown(wrapped)).toBe(true)
  })
})
