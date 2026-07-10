import { AlienError } from "@alienplatform/core"
import type { Kv } from "@alienplatform/sdk"

/**
 * Sanitize a value for use inside a KV key.
 *
 * KV keys only allow `a-z A-Z 0-9 - _ : .`; event identifiers like a cron
 * schedule (`* * * * *`) contain spaces and `*`, so every disallowed character
 * maps to `_`. Record and lookup sides must both use this so the keys match.
 */
export const sanitizeKvKeyPart = (part: string) => part.replace(/[^a-zA-Z0-9\-_:.]/g, "_")

/** Iterate every key under a prefix, following the KV scan cursor across pages. */
export async function* scanAll(store: Kv, prefix: string) {
  let cursor: string | undefined
  do {
    const page = await store.scan(prefix, undefined, cursor)
    for (const item of page.items) yield item
    cursor = page.nextCursor
  } while (cursor)
}

/**
 * Wrap an arbitrary error as a 500-level `AlienErrorOptions` (the external
 * shape used over the wire). The wrapped `message` includes the source's
 * code and message so binding-test handlers that only forward
 * `{ error: alienError.message }` still surface the real cause to the e2e
 * test runner. The full `source` chain is also attached for richer
 * inspection by callers that read it.
 */
export async function toExternalOperationError(error: unknown, operation: string) {
  const source = await AlienError.from(error)
  return new AlienError({
    code: "E2E_OPERATION_FAILED",
    // Embed source code+message so the e2e test runner's `bail!` body
    // shows the real failure, not just "Operation 'X' failed".
    message: `Operation '${operation}' failed: ${source.code}: ${source.message}`,
    retryable: source.retryable,
    internal: false,
    httpStatusCode: 500,
    context: { operation, sourceCode: source.code },
    source: source.toOptions(),
  }).toExternal()
}
