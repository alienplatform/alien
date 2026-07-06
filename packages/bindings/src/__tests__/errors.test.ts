import { AlienError } from "@alienplatform/core"
import { describe, expect, it } from "vitest"
import { BindingNotConfiguredError, unwrapNapiError } from "../errors.js"

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

  it("preserves code, context, and retryable for other envelope codes", () => {
    const err = unwrapNapiError(
      napiError({
        code: "STORAGE_OPERATION_FAILED",
        message: "get failed",
        context: { binding_name: "files", operation: "get" },
        retryable: true,
      }),
    )

    expect(err).toBeInstanceOf(AlienError)
    expect(err.code).toBe("STORAGE_OPERATION_FAILED")
    expect(err.message).toBe("get failed")
    expect(err.retryable).toBe(true)
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
