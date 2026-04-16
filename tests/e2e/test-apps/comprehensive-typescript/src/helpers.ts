import { AlienError } from "@alienplatform/core"

export async function toExternalOperationError(error: unknown, operation: string) {
  const source = await AlienError.from(error)
  return new AlienError({
    code: "E2E_OPERATION_FAILED",
    message: `Operation '${operation}' failed`,
    retryable: source.retryable,
    internal: false,
    httpStatusCode: 500,
    context: { operation },
    source: source.toOptions(),
  }).toExternal()
}
