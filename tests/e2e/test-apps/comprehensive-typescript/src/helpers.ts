import { AlienError } from "@alienplatform/core"

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
