/**
 * Presigned-transfer mechanics shared by the sender and the receiver:
 * expiration check, http vs local backend dispatch, and the `..` traversal
 * guard for local paths.
 *
 * Mechanics are identical everywhere; POLICY is explicit at each call site
 * via `allowLocal`:
 * - The sender ({@link ../client.js}) gates the local (dev-only) backend
 *   behind its `allowLocalStorage` client option, default off.
 * - The receiver passes `allowLocal: true` — receivers run inside the
 *   deployment, and the local backend is how the local platform delivers
 *   bodies.
 */

import { AlienError } from "@alienplatform/core"
import { StorageOperationFailedError } from "./errors.js"
import type { PresignedRequest } from "./protocol.js"

export interface PresignedTransferOptions {
  /** fetch implementation for http backends (default: global fetch). */
  fetchImpl?: typeof fetch
  /** Allow the local filesystem backend (see module docs for the policy). */
  allowLocal: boolean
}

type Operation = "download" | "upload"

/** URL used in error reports for a presigned request. */
function errorUrl(request: PresignedRequest): string {
  return request.backend.type === "http" ? request.backend.url : "local"
}

/** Reject a presigned request that expired before we could use it. */
function assertNotExpired(request: PresignedRequest, operation: Operation): void {
  const expiration = new Date(request.expiration)
  if (Date.now() > expiration.getTime()) {
    throw new AlienError(
      StorageOperationFailedError.create({
        operation,
        url: errorUrl(request),
        reason: `Presigned request expired at ${expiration.toISOString()}`,
      }),
    )
  }
}

/**
 * Validate a local-backend file path: enforces the `allowLocal` policy and
 * the `..` traversal guard.
 */
function requireLocalPath(filePath: string, operation: Operation, allowLocal: boolean): string {
  if (!allowLocal) {
    throw new AlienError(
      StorageOperationFailedError.create({
        operation,
        url: `local://${filePath}`,
        reason: "Local storage backend not enabled (set allowLocalStorage: true for local dev)",
      }),
    )
  }
  if (filePath.includes("..")) {
    throw new AlienError(
      StorageOperationFailedError.create({
        operation,
        url: `local://${filePath}`,
        reason: "Path traversal not allowed in local storage paths",
      }),
    )
  }
  return filePath
}

function unknownBackend(request: PresignedRequest, operation: Operation) {
  return new AlienError(
    StorageOperationFailedError.create({
      operation,
      url: "unknown",
      reason: `Unknown storage backend type: ${(request.backend as { type: string }).type}`,
    }),
  )
}

/** Download the bytes behind a presigned GET request. */
export async function downloadPresigned(
  request: PresignedRequest,
  options: PresignedTransferOptions,
): Promise<Uint8Array> {
  assertNotExpired(request, "download")

  if (request.backend.type === "http") {
    const fetchImpl = options.fetchImpl ?? fetch
    const response = await fetchImpl(request.backend.url, {
      method: request.backend.method,
      headers: request.backend.headers,
    })
    if (!response.ok) {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "download",
          url: request.backend.url,
          reason: `HTTP ${response.status} ${response.statusText}`,
        }),
      )
    }
    return new Uint8Array(await response.arrayBuffer())
  }

  if (request.backend.type !== "local") {
    throw unknownBackend(request, "download")
  }
  const filePath = requireLocalPath(request.backend.filePath, "download", options.allowLocal)
  const { readFile } = await import("node:fs/promises")
  return new Uint8Array(await readFile(filePath))
}

/** Upload bytes to the target of a presigned PUT request. */
export async function uploadPresigned(
  request: PresignedRequest,
  bytes: Uint8Array,
  options: PresignedTransferOptions,
): Promise<void> {
  assertNotExpired(request, "upload")

  if (request.backend.type === "http") {
    const fetchImpl = options.fetchImpl ?? fetch
    const response = await fetchImpl(request.backend.url, {
      method: request.backend.method,
      headers: request.backend.headers,
      body: bytes,
    })
    if (!response.ok) {
      throw new AlienError(
        StorageOperationFailedError.create({
          operation: "upload",
          url: request.backend.url,
          reason: `Storage upload failed with status ${response.status}`,
        }),
      )
    }
    return
  }

  if (request.backend.type !== "local") {
    throw unknownBackend(request, "upload")
  }
  const filePath = requireLocalPath(request.backend.filePath, "upload", options.allowLocal)
  const { writeFile } = await import("node:fs/promises")
  await writeFile(filePath, bytes)
}
