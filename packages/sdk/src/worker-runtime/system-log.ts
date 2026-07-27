/**
 * Private wire marker understood by alien-worker-runtime.
 *
 * The runtime removes this marker before forwarding the message and, when the
 * log path supports structured metadata, records `alien.system=true`.
 */
const SYSTEM_LOG_PREFIX = "\u001eALIEN_SYSTEM\u001f"

export function logSystemError(message: string, ...args: unknown[]): void {
  console.error(`${SYSTEM_LOG_PREFIX}${message}`, ...args)
}

export function logSystemWarn(message: string, ...args: unknown[]): void {
  console.warn(`${SYSTEM_LOG_PREFIX}${message}`, ...args)
}
