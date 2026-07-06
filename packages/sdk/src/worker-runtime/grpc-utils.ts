/**
 * gRPC utility functions for the Worker runtime's control-plane calls.
 *
 * The only gRPC surfaces left in the SDK are the Worker protocol services
 * (Control + WaitUntil); binding I/O no longer flows over gRPC. So this wrapper
 * maps any transport failure to a single {@link GrpcCallError} rather than the
 * per-binding error taxonomy the old binding-gRPC clients carried.
 */

import { AlienError } from "@alienplatform/core"
import { Status } from "nice-grpc"
import { GrpcCallError } from "./errors.js"

/**
 * Convert a gRPC error to an AlienError with proper chaining.
 */
export async function grpcErrorToAlienError(
  error: unknown,
  service: string,
  method: string,
): Promise<AlienError<any>> {
  const baseError = await AlienError.from(error)

  if (error && typeof error === "object" && "code" in error) {
    const grpcError = error as { code: number; details?: string; message?: string }
    const details = grpcError.details ?? grpcError.message ?? "Unknown error"
    const code = grpcError.code
    return baseError.withContext(
      GrpcCallError.create({
        service,
        method,
        grpcCode: Status[code] ?? String(code),
        details,
      }),
    )
  }

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
): Promise<T> {
  try {
    return await call()
  } catch (error) {
    throw await grpcErrorToAlienError(error, service, method)
  }
}
