/**
 * This module provides a structured error handling system similar to Rust's error types,
 * with TypeScript type safety, error chaining, and context preservation.
 *
 * @example Basic Usage
 * ```typescript
 * import { AlienError, defineError } from '@alienplatform/core'
 * import { z } from 'zod'
 *
 * // Define a type-safe error
 * const DatabaseError = defineError({
 *   code: "DATABASE_CONNECTION_FAILED",
 *   context: z.object({
 *     host: z.string(),
 *     port: z.number(),
 *     reason: z.string(),
 *   }),
 *   message: ({ host, port, reason }) => `Failed to connect to database '${host}:${port}': ${reason}`,
 *   retryable: true,
 *   internal: false,
 *   httpStatusCode: 502,
 * })
 *
 * // Use it
 * throw new AlienError(DatabaseError.create({
 *   host: "localhost",
 *   port: 5432,
 *   reason: "Connection timeout"
 * }))
 * ```
 *
 * @example Error Chaining
 * ```typescript
 * try {
 *   // Some operation that fails
 *   throw new Error("Connection refused")
 * } catch (error) {
 *   throw (await AlienError.from(error)).withContext(
 *     DatabaseError.create({
 *       host: "localhost",
 *       port: 5432,
 *       reason: "Connection refused by server"
 *     })
 *   )
 * }
 * ```
 *
 * @example External API Sanitization
 * ```typescript
 * const internalError = new AlienError(InternalApiError.create({
 *   service: "payment-processor",
 *   details: "Database password expired",
 *   traceId: "trace-12345"
 * }))
 *
 * // Safe for external APIs (sanitizes internal errors)
 * const safeResponse = internalError.toExternal()
 * ```
 */

import { serializeError } from "serialize-error"
import type { z } from "zod/v4"
import {
  type AlienError as AlienErrorOptions,
  AlienErrorSchema as AlienErrorOptionsSchema,
} from "./generated/index.js"

// Re-export the schema for external use
export { AlienErrorOptionsSchema }
export type { AlienErrorOptions }

/**
 * Base interface that all error type definitions must implement.
 *
 * This provides the structure for creating type-safe error definitions
 * with Zod validation and contextual information.
 *
 * @template TContext - Zod schema type for the error context
 */
export interface AlienErrorMetadata<TContext extends z.ZodTypeAny> {
  /** Unique error code (e.g., "DATABASE_CONNECTION_FAILED") */
  code: string
  /** Zod schema for type-safe context validation */
  context: TContext
  /** Whether this error can be retried */
  retryable: boolean
  /** Whether this error contains sensitive information */
  internal: boolean
  /** HTTP status code for API responses */
  httpStatusCode?: number
  /** Function to generate human-readable error message from context */
  message: (context: z.infer<TContext>) => string
}

/**
 * Helper function to create type-safe error definitions.
 *
 * This function provides a clean API for defining reusable error types
 * with full TypeScript type safety and Zod validation.
 *
 * @template TContext - Zod schema type for the error context
 * @param metadata - Error metadata including code, schema, and message generator
 * @returns Object with metadata and a create function for error instances
 *
 * @example
 * ```typescript
 * const UserNotFound = defineError({
 *   code: "USER_NOT_FOUND",
 *   context: z.object({
 *     userId: z.string(),
 *     searchMethod: z.string(),
 *   }),
 *   message: ({ userId, searchMethod }) =>
 *     `User '${userId}' not found using method '${searchMethod}'`,
 *   retryable: false,
 *   internal: false,
 *   httpStatusCode: 404,
 * })
 *
 * // Usage
 * const error = UserNotFound.create({
 *   userId: "123",
 *   searchMethod: "database_lookup"
 * })
 * ```
 */
export function defineError<TContext extends z.ZodTypeAny>(metadata: AlienErrorMetadata<TContext>) {
  return {
    metadata,
    contextSchema: metadata.context,
    create: (context: z.infer<TContext>): AlienErrorDefinition<TContext> => ({
      metadata,
      contextSchema: metadata.context,
      context,
      toOptions: (): AlienErrorOptions => ({
        code: metadata.code,
        message: metadata.message(context),
        retryable: metadata.retryable,
        internal: metadata.internal,
        httpStatusCode: metadata.httpStatusCode,
        context,
      }),
    }),
  }
}

/**
 * Represents a specific error instance with its context.
 *
 * This is created by calling `.create()` on an error definition
 * and contains the actual context data for a specific error occurrence.
 *
 * @template TContext - Zod schema type for the error context
 */
export interface AlienErrorDefinition<TContext extends z.ZodTypeAny> {
  metadata: AlienErrorMetadata<TContext>
  contextSchema: TContext
  context: z.infer<TContext>

  /**
   * Convert this error definition directly to AlienErrorOptions.
   *
   * This allows you to get the wire format representation
   * without creating an AlienError instance first.
   *
   * @returns AlienErrorOptions object representing this error
   */
  toOptions(): AlienErrorOptions
}

/**
 * Main AlienError class that provides structured error handling.
 *
 * This class extends the standard JavaScript Error with additional features:
 * - Type-safe context data
 * - Error chaining and source tracking
 * - Retryability and internal/external visibility flags
 * - HTTP status code mapping
 * - Sanitization for external APIs
 *
 * @template TContext - Zod schema type for the error context
 *
 * @example Basic Construction
 * ```typescript
 * const error = new AlienError(DatabaseError.create({
 *   host: "localhost",
 *   port: 5432,
 *   reason: "Timeout"
 * }))
 * ```
 *
 * @example Converting JavaScript Errors
 * ```typescript
 * try {
 *   JSON.parse("invalid json")
 * } catch (jsError) {
 *   const alienError = AlienError.from(jsError)
 *   console.log(alienError.code) // "GENERIC_ERROR"
 * }
 * ```
 *
 * @example Error Chaining
 * ```typescript
 * const contextualError = AlienError.from(new Error("Network timeout"))
 *   .withContext(DatabaseError.create({
 *     host: "localhost",
 *     port: 5432,
 *     reason: "Connection timeout"
 *   }))
 *
 * // Check error chain
 * console.log(contextualError.hasErrorCode("DATABASE_CONNECTION_FAILED")) // true
 * console.log(contextualError.toString()) // Shows full error chain
 * ```
 */
export class AlienError<TContext extends z.ZodTypeAny = z.ZodAny> extends Error {
  /** Unique error code identifying the error type */
  public readonly code: string
  /** Whether this error can be safely retried */
  public readonly retryable: boolean
  /** Whether this error contains sensitive internal information */
  public readonly internal: boolean
  /** HTTP status code for API responses */
  public readonly httpStatusCode: number
  /** Type-safe context data specific to this error */
  public readonly context?: z.infer<TContext>
  /** Source error that caused this error (for chaining) */
  public readonly source?: AlienError<any>

  constructor(input: AlienErrorDefinition<TContext> | (AlienErrorOptions & { context?: any })) {
    // Handle both error definitions and raw options
    let options: AlienErrorOptions & { context?: any }

    if ("metadata" in input && "context" in input) {
      // It's an AlienErrorDefinition
      const definition = input as AlienErrorDefinition<TContext>
      const { metadata, context: contextData } = definition
      const message = metadata.message(contextData)

      options = {
        code: metadata.code,
        message,
        retryable: metadata.retryable,
        internal: metadata.internal,
        httpStatusCode: metadata.httpStatusCode,
        context: contextData,
      }
    } else {
      // It's raw AlienErrorOptions
      options = input as AlienErrorOptions & { context?: any }
    }

    const message = options.message
    super(message, { cause: options.source })

    this.name = "AlienError"
    this.code = options.code
    this.retryable = options.retryable
    this.internal = options.internal
    this.httpStatusCode = options.httpStatusCode ?? 500
    this.context = options.context

    // Handle source construction - check if it's a valid AlienErrorOptions object
    this.source = undefined
    if (options.source && typeof options.source === "object") {
      // Check if it has the required fields to be a valid AlienErrorOptions
      if (
        "code" in options.source &&
        "message" in options.source &&
        "retryable" in options.source &&
        "internal" in options.source
      ) {
        this.source = new AlienError(options.source as AlienErrorOptions)
      }
    }
  }

  /**
   * Create an AlienError from a type-safe error definition.
   *
   * @template TContext - Zod schema type for the error context
   * @param definition - Error definition with type-safe context
   * @returns New AlienError instance
   */
  static fromDefinition<TContext extends z.ZodTypeAny>(
    definition: AlienErrorDefinition<TContext>,
  ): AlienError<TContext> {
    return new AlienError(definition)
  }

  /**
   * Create an AlienError from raw options (for compatibility).
   *
   * @param options - Raw error options conforming to AlienErrorOptions schema
   * @returns New AlienError instance
   */
  static fromOptions(options: AlienErrorOptions): AlienError {
    return new AlienError(AlienErrorOptionsSchema.parse(options))
  }

  /**
   * Helper function to get a user-friendly type name for error context.
   *
   * @param error - Any value to get the type name for
   * @returns User-friendly type name
   */
  private static getErrorTypeName(error: any): string {
    // Handle null explicitly (typeof null === "object" is a JS quirk)
    if (error === null) {
      return "null"
    }

    // Handle undefined
    if (error === undefined) {
      return "undefined"
    }

    // Try to get constructor name first (for Arrays, Functions, custom classes, etc.)
    if (error?.constructor?.name) {
      return error.constructor.name
    }

    // Fall back to typeof, but capitalize for consistency
    const typeofResult = typeof error
    return typeofResult === "object"
      ? "Object"
      : typeofResult.charAt(0).toUpperCase() + typeofResult.slice(1)
  }

  /**
   * Convert a JavaScript Error or any object to a generic AlienError.
   *
   * This is equivalent to Rust's `.into_alien_error()` pattern and provides
   * a way to convert any JavaScript error into the structured AlienError format.
   *
   * @param error - Any JavaScript error or object to convert
   * @returns AlienError with GENERIC_ERROR code and preserved original error
   *
   * @example
   * ```typescript
   * try {
   *   throw new TypeError("Invalid argument")
   * } catch (error) {
   *   const alienError = AlienError.from(error)
   *   console.log(alienError.code) // "GENERIC_ERROR"
   *   console.log(alienError.context?.originalError) // Serialized TypeError
   * }
   * ```
   *
   * @example Chaining with context
   * ```typescript
   * try {
   *   JSON.parse("invalid")
   * } catch (error) {
   *   throw AlienError.from(error).withContext(
   *     ValidationError.create({
   *       input: "invalid",
   *       expectedFormat: "JSON"
   *     })
   *   )
   * }
   * ```
   *
   * @example AxiosError conversion
   * ```typescript
   * try {
   *   await axios.get('/api/resource')
   * } catch (error) {
   *   // If error.response.data contains a valid AlienError structure, it will be parsed
   *   const alienError = AlienError.from(error)
   *   console.log(alienError.code) // Could be "REMOTE_RESOURCE_NOT_FOUND" if parsed from response
   * }
   * ```
   */
  static async from(error: any): Promise<AlienError> {
    if (error instanceof AlienError) {
      return error
    }

    // Try to parse as fetch Response with AlienError in body
    if (error instanceof Response) {
      let errorBody: any
      try {
        errorBody = await error.json()
      } catch {
        // If JSON parsing fails, create generic error
        return new AlienError({
          code: "GENERIC_ERROR",
          message: `HTTP request failed with status ${error.status}`,
          retryable: false,
          internal: false,
          httpStatusCode: error.status,
          context: {
            url: error.url,
            status: error.status,
            statusText: error.statusText,
          },
        })
      }

      // Try to parse as AlienError
      const parseResult = AlienErrorOptionsSchema.safeParse(errorBody)
      if (parseResult.success) {
        return new AlienError(parseResult.data)
      }

      // If not an AlienError, wrap it as generic error
      return new AlienError({
        code: "GENERIC_ERROR",
        message: errorBody?.message || `HTTP request failed with status ${error.status}`,
        retryable: false,
        internal: false,
        httpStatusCode: error.status,
        context: {
          url: error.url,
          status: error.status,
          statusText: error.statusText,
          responseBody: errorBody,
        },
      })
    }

    // Try to parse as AxiosError with AlienError in response.data
    if (error?.response?.data && typeof error.response.data === "object") {
      const parseResult = AlienErrorOptionsSchema.safeParse(error.response.data)
      if (parseResult.success) {
        return new AlienError(parseResult.data)
      }
      // If parsing fails, continue with generic error handling
      // This is expected for non-AlienError responses
    }

    // Serialize the error using serialize-error for consistent structure
    const serialized = serializeError(error)
    const message = serialized?.message || String(error)

    return new AlienError({
      code: "GENERIC_ERROR",
      message,
      retryable: false,
      internal: false,
      httpStatusCode: 500,
      context: {
        originalError: serialized,
        errorType: AlienError.getErrorTypeName(error),
      },
    })
  }

  /**
   * Add context to this error, creating a new error that wraps this one.
   *
   * This is equivalent to Rust's `.with_context()` pattern and allows
   * building error chains with increasing levels of context.
   *
   * @template TNewContext - Zod schema type for the new context
   * @param definition - Error definition to wrap this error with
   * @returns New AlienError with this error as the source
   *
   * @example
   * ```typescript
   * // Clean fluent API
   * const authError = (await AlienError.from(new Error("ECONNREFUSED")))
   *   .withContext(DatabaseError.create({
   *     host: "localhost",
   *     port: 5432,
   *     reason: "Connection refused"
   *   }))
   *   .withContext(AuthError.create({
   *     username: "john",
   *     reason: "Database unavailable"
   *   }))
   *
   * // Error chain: AuthError -> DatabaseError -> GENERIC_ERROR
   * console.log(authError.toString())
   * ```
   */
  withContext<TNewContext extends z.ZodTypeAny>(
    definition: AlienErrorDefinition<TNewContext>,
  ): AlienError<TNewContext> {
    const { metadata, context: contextData } = definition
    const message = metadata.message(contextData)

    // Convert current error to source format
    const sourceOptions: AlienErrorOptions = {
      code: this.code,
      message: this.message,
      retryable: this.retryable,
      internal: this.internal,
      httpStatusCode: this.httpStatusCode,
      context: this.context,
      source: this.source?.toOptions(),
    }

    return new AlienError({
      code: metadata.code,
      message,
      retryable: metadata.retryable,
      internal: metadata.internal,
      httpStatusCode: metadata.httpStatusCode,
      context: contextData,
      source: sourceOptions,
    })
  }

  /**
   * Convert this AlienError to the wire format (AlienErrorOptions).
   *
   * This method serializes the error into the standard format used
   * for transmitting errors over the network or storing them.
   *
   * @returns AlienErrorOptions object representing this error
   */
  toOptions(): AlienErrorOptions {
    return {
      code: this.code,
      message: this.message,
      retryable: this.retryable,
      internal: this.internal,
      httpStatusCode: this.httpStatusCode,
      context: this.context,
      source: this.source?.toOptions(),
    }
  }

  /**
   * Get a sanitized version for external APIs (hides internal errors).
   *
   * This method is crucial for security - it prevents sensitive internal
   * error details from being exposed to external users while preserving
   * the error chain structure for non-sensitive errors.
   *
   * @returns Sanitized AlienErrorOptions safe for external consumption
   *
   * @example
   * ```typescript
   * const internalError = new AlienError(InternalApiError.create({
   *   service: "payment-processor",
   *   details: "Database password expired for user admin",
   *   traceId: "trace-12345"
   * }))
   *
   * console.log(internalError.toOptions())
   * // Shows full details including sensitive information
   *
   * console.log(internalError.toExternal())
   * // { code: "GENERIC_ERROR", message: "Internal server error", ... }
   * ```
   */
  toExternal(): AlienErrorOptions {
    if (this.internal) {
      return {
        code: "GENERIC_ERROR",
        message: "Internal server error",
        retryable: false,
        internal: false,
        httpStatusCode: 500,
      }
    }

    return {
      code: this.code,
      message: this.message,
      retryable: this.retryable,
      internal: this.internal,
      httpStatusCode: this.httpStatusCode,
      context: this.context,
      source: this.source?.toExternal(),
    }
  }

  /**
   * Check if this error chain contains an error with a specific code.
   *
   * This method traverses the entire error chain (this error and all
   * source errors) to find if any error has the specified code.
   *
   * @param code - Error code to search for
   * @returns True if the code is found anywhere in the error chain
   *
   * @example
   * ```typescript
   * const chainedError = authError.withContext(dbError.withContext(networkError))
   *
   * console.log(chainedError.hasErrorCode("AUTH_FAILED")) // true
   * console.log(chainedError.hasErrorCode("DATABASE_CONNECTION_FAILED")) // true
   * console.log(chainedError.hasErrorCode("NETWORK_TIMEOUT")) // true
   * console.log(chainedError.hasErrorCode("NONEXISTENT_ERROR")) // false
   * ```
   */
  hasErrorCode(code: string): boolean {
    if (this.code === code) return true
    return this.source?.hasErrorCode(code) ?? false
  }

  /**
   * Find the first error in the chain with a specific code.
   *
   * This method traverses the error chain and returns the first error
   * instance that matches the specified code, allowing access to its
   * specific context and details.
   *
   * @param code - Error code to search for
   * @returns AlienError instance with the specified code, or undefined if not found
   *
   * @example
   * ```typescript
   * const chainedError = authError.withContext(dbError)
   *
   * const dbError = chainedError.findErrorByCode("DATABASE_CONNECTION_FAILED")
   * if (dbError) {
   *   console.log("Database error context:", dbError.context)
   *   console.log("Host:", dbError.context?.host)
   * }
   * ```
   */
  findErrorByCode(code: string): AlienError | undefined {
    if (this.code === code) return this
    return this.source?.findErrorByCode(code)
  }

  /**
   * Get a formatted string representation of the error chain.
   *
   * This method provides a human-readable representation of the entire
   * error chain, showing how errors are nested and providing context
   * for debugging and logging.
   *
   * @returns Formatted string showing the complete error chain
   *
   * @example
   * ```typescript
   * const chainedError = authError.withContext(dbError.withContext(networkError))
   * console.log(chainedError.toString())
   * // Output:
   * // AUTH_FAILED: User authentication failed for 'john'
   * //   ├─▶ DATABASE_CONNECTION_FAILED: Failed to connect to database 'localhost:5432'
   * //     ├─▶ GENERIC_ERROR: Connection timeout
   * ```
   */
  toString(): string {
    let result = `${this.code}: ${this.message}`
    let current = this.source
    let indent = ""

    while (current) {
      indent += "  "
      result += `\n${indent}├─▶ ${current.code}: ${current.message}`
      current = current.source
    }

    return result
  }
}

// Utility type to extract context type from error definition
export type ExtractContext<T> = T extends AlienErrorDefinition<infer TContext>
  ? z.infer<TContext>
  : never
