import { AlienError } from "@alienplatform/core"

export function failCheck(
  check: string,
  message: string,
  details?: Record<string, unknown>,
): never {
  throw new AlienError({
    code: "E2E_CHECK_FAILED",
    message: `E2E check '${check}' failed: ${message}`,
    retryable: false,
    internal: false,
    httpStatusCode: 500,
    context: { check, details },
  })
}

export function assertCheck(
  condition: unknown,
  check: string,
  message: string,
  details?: Record<string, unknown>,
): asserts condition {
  if (!condition) {
    failCheck(check, message, details)
  }
}

export async function assertResponseOk(
  response: Response,
  check: string,
  message: string,
): Promise<void> {
  if (response.ok) {
    return
  }
  const body = await response.text()
  failCheck(check, message, { status: response.status, body })
}
