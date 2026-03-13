import * as z from "zod/v4"
import { defineError } from "./error.js"

/**
 * Error thrown when a requested resource cannot be found in the stack state.
 * This matches the Rust alien-core error: RESOURCE_NOT_FOUND
 */
export const ResourceNotFoundError = defineError({
  code: "RESOURCE_NOT_FOUND",
  context: z.object({
    resourceId: z.string(),
    availableResources: z.array(z.string()),
  }),
  message: ({ resourceId, availableResources }) =>
    `Resource '${resourceId}' not found in stack state. Available resources: ${JSON.stringify(availableResources)}`,
  retryable: false,
  internal: false,
  httpStatusCode: 404,
})

/**
 * Error thrown when resource outputs cannot be parsed according to their expected schema.
 * This indicates an internal system error or data corruption.
 */
export const ResourceOutputsParseError = defineError({
  code: "RESOURCE_OUTPUTS_PARSE_ERROR",
  context: z.object({
    resourceName: z.string(),
    resourceType: z.string(),
    validationErrors: z.string(),
  }),
  message: ({ resourceName, resourceType, validationErrors }) =>
    `Failed to parse outputs for resource '${resourceName}' of type '${resourceType}':\n${validationErrors}`,
  retryable: false,
  internal: true,
  httpStatusCode: 500,
})

/**
 * Error thrown when there's a resource type mismatch during stack operations.
 * This matches the Rust alien-core error: UNEXPECTED_RESOURCE_TYPE
 */
export const UnexpectedResourceTypeError = defineError({
  code: "UNEXPECTED_RESOURCE_TYPE",
  context: z.object({
    resourceId: z.string(),
    expected: z.string(),
    actual: z.string(),
  }),
  message: ({ resourceId, expected, actual }) =>
    `Unexpected resource type for resource '${resourceId}': expected ${expected}, but got ${actual}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})
