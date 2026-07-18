/**
 * Wire-response validation.
 *
 * The command server's JSON responses are validated at the fetch boundary
 * against the Kubb-generated zod schemas in `@alienplatform/core`. A body that
 * doesn't match its schema becomes a typed {@link MalformedResponseError} here,
 * rather than a `TypeError` thrown deep in the caller when a missing field is
 * later dereferenced.
 */

import { AlienError } from "@alienplatform/core"
import { MalformedResponseError } from "./errors.js"

/**
 * Minimal structural view of a zod schema — just the `safeParse` we use. Lets
 * this helper accept the core schemas without importing zod (a dev-only
 * dependency) into the published type surface.
 */
export interface WireSchema<T> {
  safeParse(
    value: unknown,
  ): { success: true; data: T } | { success: false; error: { message: string } }
}

/**
 * Validate a decoded JSON response against `schema`, returning the typed value
 * or throwing a {@link MalformedResponseError} naming the request that produced
 * the malformed body.
 */
export function parseWireResponse<T>(
  schema: WireSchema<T>,
  value: unknown,
  method: string,
  url: string,
): T {
  const result = schema.safeParse(value)
  if (result.success) {
    return result.data
  }
  throw new AlienError(MalformedResponseError.create({ method, url, reason: result.error.message }))
}
