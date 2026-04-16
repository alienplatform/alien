/**
 * gRPC utility functions for error handling and type conversion.
 */

import { AlienError } from "@alienplatform/core"
import { Status } from "nice-grpc"
import {
  BindingNotFoundError,
  GrpcCallError,
  KvInvalidKeyError,
  KvInvalidValueError,
  KvKeyNotFoundError,
  SecretNotFoundError,
  StorageObjectExistsError,
  StorageObjectNotFoundError,
  StoragePreconditionError,
} from "./errors.js"

/**
 * Convert a gRPC error to an AlienError with proper chaining.
 */
export async function grpcErrorToAlienError(
  error: unknown,
  service: string,
  method: string,
  context?: {
    bindingName?: string
    path?: string
    key?: string
    secretName?: string
    bindingType?: string
  },
): Promise<AlienError<any>> {
  // Start with the original error wrapped as AlienError
  const baseError = await AlienError.from(error)

  // Handle nice-grpc errors
  if (error && typeof error === "object" && "code" in error) {
    const grpcError = error as { code: number; details?: string; message?: string }
    const details = grpcError.details ?? grpcError.message ?? "Unknown error"
    const code = grpcError.code

    // Map specific gRPC status codes to domain errors
    if (code === Status.NOT_FOUND) {
      // Determine the appropriate not-found error based on context
      if (context?.path && context?.bindingName) {
        return baseError.withContext(
          StorageObjectNotFoundError.create({
            bindingName: context.bindingName,
            path: context.path,
          }),
        )
      }
      if (context?.key && context?.bindingName) {
        return baseError.withContext(
          KvKeyNotFoundError.create({
            bindingName: context.bindingName,
            key: context.key,
          }),
        )
      }
      if (context?.secretName && context?.bindingName) {
        return baseError.withContext(
          SecretNotFoundError.create({
            bindingName: context.bindingName,
            secretName: context.secretName,
          }),
        )
      }
      if (context?.bindingName && context?.bindingType) {
        return baseError.withContext(
          BindingNotFoundError.create({
            bindingName: context.bindingName,
            bindingType: context.bindingType,
          }),
        )
      }
    }

    if (code === Status.FAILED_PRECONDITION && context?.path && context?.bindingName) {
      return baseError.withContext(
        StoragePreconditionError.create({
          bindingName: context.bindingName,
          path: context.path,
          condition: details,
        }),
      )
    }

    if (code === Status.ALREADY_EXISTS && context?.path && context?.bindingName) {
      return baseError.withContext(
        StorageObjectExistsError.create({
          bindingName: context.bindingName,
          path: context.path,
        }),
      )
    }

    if (code === Status.INVALID_ARGUMENT) {
      if (context?.key) {
        return baseError.withContext(
          KvInvalidKeyError.create({
            key: context.key,
            reason: details,
          }),
        )
      }
      if (service === "KvService") {
        return baseError.withContext(
          KvInvalidValueError.create({
            reason: details,
          }),
        )
      }
    }

    // Generic gRPC error
    return baseError.withContext(
      GrpcCallError.create({
        service,
        method,
        grpcCode: Status[code] ?? String(code),
        details,
      }),
    )
  }

  // Fallback for non-gRPC errors
  return baseError.withContext(
    GrpcCallError.create({
      service,
      method,
      grpcCode: "UNKNOWN",
      details: error instanceof Error ? error.message : String(error),
    }),
  )
}

/**
 * Wrap an async gRPC call with error handling.
 */
export async function wrapGrpcCall<T>(
  service: string,
  method: string,
  call: () => Promise<T>,
  context?: {
    bindingName?: string
    path?: string
    key?: string
    secretName?: string
    bindingType?: string
  },
): Promise<T> {
  try {
    return await call()
  } catch (error) {
    throw await grpcErrorToAlienError(error, service, method, context)
  }
}
