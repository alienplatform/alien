import { describe, expect, it } from "vitest"
import { z } from "zod/v4"

import { AlienError, type AlienErrorOptions, defineError } from "../error.js"

// Define test error types similar to the Rust examples
const DatabaseError = defineError({
  code: "DATABASE_CONNECTION_FAILED",
  context: z.object({
    host: z.string(),
    port: z.number(),
    reason: z.string(),
  }),
  message: ({ host, port, reason }) => `Failed to connect to database '${host}:${port}': ${reason}`,
  retryable: true,
  internal: false,
  httpStatusCode: 502,
})

const AuthError = defineError({
  code: "AUTH_FAILED",
  context: z.object({
    username: z.string(),
    reason: z.string(),
  }),
  message: ({ username, reason }) => `Authentication failed for user '${username}': ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 401,
})

const InternalApiError = defineError({
  code: "INTERNAL_API_ERROR",
  context: z.object({
    service: z.string(),
    details: z.string(),
    traceId: z.string(),
  }),
  message: ({ service, details }) => `Internal API error in ${service}: ${details}`,
  retryable: false,
  internal: true, // This is internal and should be sanitized
  httpStatusCode: 500,
})

const ValidationError = defineError({
  code: "VALIDATION_ERROR",
  context: z.object({
    field: z.string(),
    value: z.any(),
    expectedType: z.string(),
  }),
  message: ({ field, expectedType }) =>
    `Validation failed for field '${field}': expected ${expectedType}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

const EmptyContextError = defineError({
  code: "EMPTY_CONTEXT_ERROR",
  context: z.object({}),
  message: () => "Error with no context fields",
  retryable: false,
  internal: false,
})

describe("AlienError Basic Usage", () => {
  it("creates an error from definition", () => {
    const error = new AlienError(
      DatabaseError.create({
        host: "localhost",
        port: 5432,
        reason: "Connection timeout",
      }),
    )

    expect(error.code).toBe("DATABASE_CONNECTION_FAILED")
    expect(error.message).toBe("Failed to connect to database 'localhost:5432': Connection timeout")
    expect(error.retryable).toBe(true)
    expect(error.internal).toBe(false)
    expect(error.httpStatusCode).toBe(502)
    expect(error.context).toEqual({
      host: "localhost",
      port: 5432,
      reason: "Connection timeout",
    })
    expect(error.source).toBeUndefined()
  })

  it("creates an error with empty context", () => {
    const error = new AlienError(EmptyContextError.create({}))

    expect(error.code).toBe("EMPTY_CONTEXT_ERROR")
    expect(error.message).toBe("Error with no context fields")
    expect(error.context).toEqual({})
  })

  it("fromDefinition static method works", () => {
    const error = AlienError.fromDefinition(
      AuthError.create({
        username: "john",
        reason: "Invalid password",
      }),
    )

    expect(error.code).toBe("AUTH_FAILED")
    expect(error.message).toBe("Authentication failed for user 'john': Invalid password")
    expect(error.retryable).toBe(false)
    expect(error.httpStatusCode).toBe(401)
  })

  it("toOptions converts to wire format", () => {
    const error = new AlienError(
      ValidationError.create({
        field: "email",
        value: "invalid-email",
        expectedType: "valid email address",
      }),
    )

    const options = error.toOptions()

    expect(options).toEqual({
      code: "VALIDATION_ERROR",
      message: "Validation failed for field 'email': expected valid email address",
      retryable: false,
      internal: false,
      httpStatusCode: 400,
      context: {
        field: "email",
        value: "invalid-email",
        expectedType: "valid email address",
      },
      source: undefined,
    })
  })

  it("error definition toOptions converts directly to wire format", () => {
    const definition = ValidationError.create({
      field: "email",
      value: "invalid-email",
      expectedType: "valid email address",
    })

    const options = definition.toOptions()

    expect(options).toEqual({
      code: "VALIDATION_ERROR",
      message: "Validation failed for field 'email': expected valid email address",
      retryable: false,
      internal: false,
      httpStatusCode: 400,
      context: {
        field: "email",
        value: "invalid-email",
        expectedType: "valid email address",
      },
    })
  })
})

describe("AlienError.from() with JS Error types", () => {
  it("converts basic Error", async () => {
    const jsError = new Error("Something went wrong")
    const alienError = await AlienError.from(jsError)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("Something went wrong")
    expect(alienError.retryable).toBe(false)
    expect(alienError.internal).toBe(false)
    expect(alienError.httpStatusCode).toBe(500)
    expect(alienError.context?.originalError).toEqual({
      name: "Error",
      message: "Something went wrong",
      stack: expect.any(String),
    })
    expect(alienError.context?.errorType).toBe("Error")
  })

  it("converts TypeError", async () => {
    const jsError = new TypeError("Cannot read property 'foo' of undefined")
    const alienError = await AlienError.from(jsError)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("Cannot read property 'foo' of undefined")
    expect(alienError.context?.originalError.name).toBe("TypeError")
    expect(alienError.context?.errorType).toBe("TypeError")
  })

  it("converts ReferenceError", async () => {
    const jsError = new ReferenceError("foo is not defined")
    const alienError = await AlienError.from(jsError)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("foo is not defined")
    expect(alienError.context?.originalError.name).toBe("ReferenceError")
    expect(alienError.context?.errorType).toBe("ReferenceError")
  })

  it("converts SyntaxError", async () => {
    const jsError = new SyntaxError("Unexpected token")
    const alienError = await AlienError.from(jsError)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("Unexpected token")
    expect(alienError.context?.originalError.name).toBe("SyntaxError")
    expect(alienError.context?.errorType).toBe("SyntaxError")
  })

  it("converts RangeError", async () => {
    const jsError = new RangeError("Maximum call stack size exceeded")
    const alienError = await AlienError.from(jsError)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("Maximum call stack size exceeded")
    expect(alienError.context?.originalError.name).toBe("RangeError")
    expect(alienError.context?.errorType).toBe("RangeError")
  })

  it("returns same AlienError when passed AlienError", async () => {
    const originalError = new AlienError(
      DatabaseError.create({
        host: "localhost",
        port: 5432,
        reason: "Timeout",
      }),
    )

    const result = await AlienError.from(originalError)

    expect(result).toBe(originalError) // Should be the exact same instance
    expect(result.code).toBe("DATABASE_CONNECTION_FAILED")
  })

  it("handles errors with custom properties", async () => {
    const jsError = new Error("Custom error") as any
    jsError.customProp = "custom value"
    jsError.errorCode = 42

    const alienError = await AlienError.from(jsError)

    expect(alienError.context?.originalError).toEqual({
      name: "Error",
      message: "Custom error",
      stack: expect.any(String),
      customProp: "custom value",
      errorCode: 42,
    })
  })
})

describe("AlienError.from() with random JS objects", async () => {
  it("converts simple error object", async () => {
    const errorObj = { error: "Something bad happened" }
    const alienError = await AlienError.from(errorObj)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("[object Object]") // Default toString behavior
    expect(alienError.context?.originalError).toEqual(errorObj)
    expect(alienError.context?.errorType).toBe("Object")
  })

  it("converts string", async () => {
    const errorString = "Just a string error"
    const alienError = await AlienError.from(errorString)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("Just a string error")
    expect(alienError.context?.originalError).toBe(errorString)
    expect(alienError.context?.errorType).toBe("String")
  })

  it("converts number", async () => {
    const errorNumber = 404
    const alienError = await AlienError.from(errorNumber)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("404")
    expect(alienError.context?.originalError).toBe(404)
    expect(alienError.context?.errorType).toBe("Number")
  })

  it("converts null", async () => {
    const alienError = await AlienError.from(null)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("null")
    expect(alienError.context?.originalError).toBe(null)
    expect(alienError.context?.errorType).toBe("null")
  })

  it("converts undefined", async () => {
    const alienError = await AlienError.from(undefined)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.message).toBe("undefined")
    expect(alienError.context?.originalError).toBe(undefined)
    expect(alienError.context?.errorType).toBe("undefined")
  })

  it("converts complex object with nested data", async () => {
    const complexObj = {
      error: "Database connection failed",
      details: {
        host: "db.example.com",
        port: 5432,
        timeout: 30000,
      },
      timestamp: "2023-10-01T12:00:00Z",
      retryCount: 3,
    }

    const alienError = await AlienError.from(complexObj)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.context?.originalError).toEqual(complexObj)
    expect(alienError.context?.errorType).toBe("Object")
  })

  it("converts array", async () => {
    const errorArray = ["error1", "error2", { message: "error3" }]
    const alienError = await AlienError.from(errorArray)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.context?.originalError).toEqual(errorArray)
    expect(alienError.context?.errorType).toBe("Array") // Arrays are objects in JS
  })

  it("converts function", async () => {
    const errorFunc = () => "error"
    const alienError = await AlienError.from(errorFunc)

    expect(alienError.code).toBe("GENERIC_ERROR")
    expect(alienError.context?.errorType).toBe("Function")
    // Note: Functions get serialized differently by serialize-error
  })
})

describe("Error Chaining", async () => {
  it("chains errors with withContext", async () => {
    const baseError = new Error("Network timeout")

    const chainedError = (await AlienError.from(baseError)).withContext(
      DatabaseError.create({
        host: "localhost",
        port: 5432,
        reason: "Connection timeout",
      }),
    )

    expect(chainedError.code).toBe("DATABASE_CONNECTION_FAILED")
    expect(chainedError.message).toBe(
      "Failed to connect to database 'localhost:5432': Connection timeout",
    )
    expect(chainedError.retryable).toBe(true)
    expect(chainedError.internal).toBe(false)

    // Check source
    expect(chainedError.source).toBeDefined()
    expect(chainedError.source?.code).toBe("GENERIC_ERROR")
    expect(chainedError.source?.message).toBe("Network timeout")
  })

  it("chains multiple errors", async () => {
    const baseError = new Error("ECONNREFUSED")

    const multiChainedError = (await AlienError.from(baseError))
      .withContext(
        DatabaseError.create({
          host: "localhost",
          port: 5432,
          reason: "Connection refused",
        }),
      )
      .withContext(
        AuthError.create({
          username: "john",
          reason: "Database unavailable",
        }),
      )

    expect(multiChainedError.code).toBe("AUTH_FAILED")
    expect(multiChainedError.message).toBe(
      "Authentication failed for user 'john': Database unavailable",
    )

    // Check first level source
    expect(multiChainedError.source?.code).toBe("DATABASE_CONNECTION_FAILED")

    // Check second level source
    expect(multiChainedError.source?.source?.code).toBe("GENERIC_ERROR")
    expect(multiChainedError.source?.source?.message).toBe("ECONNREFUSED")
  })

  it("hasErrorCode works with chains", async () => {
    const chainedError = (await AlienError.from(new Error("base")))
      .withContext(
        DatabaseError.create({
          host: "localhost",
          port: 5432,
          reason: "timeout",
        }),
      )
      .withContext(
        AuthError.create({
          username: "john",
          reason: "db issues",
        }),
      )

    expect(chainedError.hasErrorCode("AUTH_FAILED")).toBe(true)
    expect(chainedError.hasErrorCode("DATABASE_CONNECTION_FAILED")).toBe(true)
    expect(chainedError.hasErrorCode("GENERIC_ERROR")).toBe(true)
    expect(chainedError.hasErrorCode("NONEXISTENT_ERROR")).toBe(false)
  })

  it("findErrorByCode works with chains", async () => {
    const chainedError = (await AlienError.from(new Error("base")))
      .withContext(
        DatabaseError.create({
          host: "localhost",
          port: 5432,
          reason: "timeout",
        }),
      )
      .withContext(
        AuthError.create({
          username: "john",
          reason: "db issues",
        }),
      )

    const authError = chainedError.findErrorByCode("AUTH_FAILED")
    expect(authError).toBeDefined()
    expect(authError?.code).toBe("AUTH_FAILED")
    expect(authError?.context?.username).toBe("john")

    const dbError = chainedError.findErrorByCode("DATABASE_CONNECTION_FAILED")
    expect(dbError).toBeDefined()
    expect(dbError?.code).toBe("DATABASE_CONNECTION_FAILED")
    expect(dbError?.context?.host).toBe("localhost")

    const nonexistent = chainedError.findErrorByCode("NONEXISTENT_ERROR")
    expect(nonexistent).toBeUndefined()
  })

  it("toString shows full error chain", async () => {
    const chainedError = (await AlienError.from(new Error("Network timeout")))
      .withContext(
        DatabaseError.create({
          host: "localhost",
          port: 5432,
          reason: "Connection timeout",
        }),
      )
      .withContext(
        AuthError.create({
          username: "john",
          reason: "Database unavailable",
        }),
      )

    const errorString = chainedError.toString()

    expect(errorString).toContain(
      "AUTH_FAILED: Authentication failed for user 'john': Database unavailable",
    )
    expect(errorString).toContain(
      "├─▶ DATABASE_CONNECTION_FAILED: Failed to connect to database 'localhost:5432': Connection timeout",
    )
    expect(errorString).toContain("├─▶ GENERIC_ERROR: Network timeout")
  })
})

describe("External API Sanitization", async () => {
  it("sanitizes internal errors for external APIs", async () => {
    const internalError = new AlienError(
      InternalApiError.create({
        service: "payment-processor",
        details: "Database password expired for user admin",
        traceId: "trace-12345",
      }),
    )

    const external = internalError.toExternal()

    expect(external).toEqual({
      code: "GENERIC_ERROR",
      message: "Internal server error",
      retryable: false,
      internal: false,
      httpStatusCode: 500,
    })
  })

  it("preserves non-internal errors for external APIs", async () => {
    const publicError = new AlienError(
      ValidationError.create({
        field: "email",
        value: "invalid-email",
        expectedType: "valid email address",
      }),
    )

    const external = publicError.toExternal()

    expect(external).toEqual({
      code: "VALIDATION_ERROR",
      message: "Validation failed for field 'email': expected valid email address",
      retryable: false,
      internal: false,
      httpStatusCode: 400,
      context: {
        field: "email",
        value: "invalid-email",
        expectedType: "valid email address",
      },
      source: undefined,
    })
  })

  it("sanitizes internal errors in error chains", async () => {
    const chainedError = (await AlienError.from(new Error("Network issue")))
      .withContext(
        InternalApiError.create({
          service: "auth-service",
          details: "JWT secret key leaked in logs",
          traceId: "trace-456",
        }),
      )
      .withContext(
        ValidationError.create({
          field: "token",
          value: "invalid-token",
          expectedType: "valid JWT",
        }),
      )

    const external = chainedError.toExternal()

    // Top level should be preserved (not internal)
    expect(external.code).toBe("VALIDATION_ERROR")
    expect(external.message).toBe("Validation failed for field 'token': expected valid JWT")
    expect(external.context).toEqual({
      field: "token",
      value: "invalid-token",
      expectedType: "valid JWT",
    })

    // But source should be sanitized (internal error)
    expect(external.source).toEqual({
      code: "GENERIC_ERROR",
      message: "Internal server error",
      retryable: false,
      internal: false,
      httpStatusCode: 500,
    })
  })

  it("preserves non-internal errors deep in chain", async () => {
    const chainedError = (await AlienError.from(new Error("Network timeout")))
      .withContext(
        DatabaseError.create({
          host: "localhost",
          port: 5432,
          reason: "Connection timeout",
        }),
      )
      .withContext(
        AuthError.create({
          username: "john",
          reason: "Database unavailable",
        }),
      )

    const external = chainedError.toExternal()

    // All errors in this chain are non-internal, so should be preserved
    expect(external.code).toBe("AUTH_FAILED")
    expect((external.source as AlienErrorOptions)?.code).toBe("DATABASE_CONNECTION_FAILED")
    expect(((external.source as AlienErrorOptions)?.source as AlienErrorOptions)?.code).toBe(
      "GENERIC_ERROR",
    )
    expect(((external.source as AlienErrorOptions)?.source as AlienErrorOptions)?.message).toBe(
      "Network timeout",
    )
  })

  it("handles mixed internal/external in chain", async () => {
    const chainedError = (await AlienError.from(new Error("Base error")))
      .withContext(
        AuthError.create({
          // Not internal
          username: "john",
          reason: "Auth failed",
        }),
      )
      .withContext(
        InternalApiError.create({
          // Internal - should be sanitized
          service: "payment",
          details: "Secret API key exposed",
          traceId: "trace-789",
        }),
      )

    const external = chainedError.toExternal()

    // Top level is internal, so gets sanitized
    expect(external).toEqual({
      code: "GENERIC_ERROR",
      message: "Internal server error",
      retryable: false,
      internal: false,
      httpStatusCode: 500,
    })
  })
})

describe("Error Metadata and Properties", () => {
  it("preserves all metadata correctly", () => {
    const error = new AlienError(
      DatabaseError.create({
        host: "db.example.com",
        port: 3306,
        reason: "SSL handshake failed",
      }),
    )

    expect(error.name).toBe("AlienError")
    expect(error.code).toBe("DATABASE_CONNECTION_FAILED")
    expect(error.retryable).toBe(true)
    expect(error.internal).toBe(false)
    expect(error.httpStatusCode).toBe(502)
    expect(error.message).toBe(
      "Failed to connect to database 'db.example.com:3306': SSL handshake failed",
    )
    expect(error.context).toEqual({
      host: "db.example.com",
      port: 3306,
      reason: "SSL handshake failed",
    })
  })

  it("handles errors without HTTP status codes", () => {
    const SimpleError = defineError({
      code: "SIMPLE_ERROR",
      context: z.object({
        message: z.string(),
      }),
      message: ({ message }) => message,
      retryable: false,
      internal: false,
      // No httpStatusCode specified
    })

    const error = new AlienError(
      SimpleError.create({
        message: "Simple error message",
      }),
    )

    expect(error.httpStatusCode).toBe(500) // Should default to 500
  })
})
