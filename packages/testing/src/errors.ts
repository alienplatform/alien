import { AlienError, defineError } from "@alienplatform/core"
import * as z from "zod/v4"

export const TestingOperationFailedError = defineError({
  code: "TESTING_OPERATION_FAILED",
  context: z.object({
    operation: z.string(),
    message: z.string(),
    details: z.record(z.string(), z.unknown()).optional(),
  }),
  message: ({ operation, message }) => `Testing operation '${operation}' failed: ${message}`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

export const TestingUnsupportedPlatformError = defineError({
  code: "TESTING_UNSUPPORTED_PLATFORM",
  context: z.object({
    platform: z.string(),
    operation: z.string(),
  }),
  message: ({ platform, operation }) =>
    `Unsupported platform '${platform}' for testing operation '${operation}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

export async function withTestingContext(
  error: unknown,
  operation: string,
  message: string,
  details?: Record<string, unknown>,
): Promise<AlienError<any>> {
  return (await AlienError.from(error)).withContext(
    TestingOperationFailedError.create({
      operation,
      message,
      details,
    }),
  )
}
