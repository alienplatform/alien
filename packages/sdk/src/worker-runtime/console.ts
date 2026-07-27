import { formatWithOptions } from "node:util"

const FORMAT_OPTIONS = {
  breakLength: Number.POSITIVE_INFINITY,
  colors: false,
  compact: true,
} as const

let installed = false

/** @internal exported for tests */
export function formatWorkerConsoleLine(args: unknown[]): string {
  try {
    return formatWithOptions(FORMAT_OPTIONS, ...args).replace(/\s*\r?\n\s*/g, " ")
  } catch {
    return "[alien:console] Log arguments could not be formatted"
  }
}

/**
 * Keep every console call within one stdout/stderr record.
 *
 * The generated Worker bootstrap imports this runtime before it dynamically
 * imports the application, so the console contract covers application module
 * initialization as well as task handling.
 */
export function installWorkerConsole(): void {
  if (installed) return
  installed = true

  const log = console.log.bind(console)
  const info = console.info.bind(console)
  const warn = console.warn.bind(console)
  const error = console.error.bind(console)

  console.log = (...args: unknown[]) => log(formatWorkerConsoleLine(args))
  console.info = (...args: unknown[]) => info(formatWorkerConsoleLine(args))
  console.warn = (...args: unknown[]) => warn(formatWorkerConsoleLine(args))
  console.error = (...args: unknown[]) => error(formatWorkerConsoleLine(args))
}
