import type { ChildProcess } from "node:child_process"

export function prepareQueueServer(): string

export function startQueueServer(
  dir: string,
  names: string[],
): {
  process: ChildProcess
  bindings: Record<string, unknown>
}
