/**
 * Command handling for Alien bindings.
 *
 * Provides command registration and execution.
 */

/**
 * Command definition.
 */
export interface CommandDefinition {
  /** Command name */
  name: string
  /** Handler function that receives params and returns a result */
  handler: (params: unknown) => Promise<unknown>
}

// Internal storage for commands
const commands: Map<string, CommandDefinition> = new Map()

/**
 * Register a command.
 *
 * @param name - Command name
 * @param handler - Command handler function that receives params and returns a result
 *
 * @example
 * ```typescript
 * import { command } from "@alienplatform/bindings"
 *
 * command("echo", async ({ message }: { message: string }) => {
 *   return {
 *     message,
 *     timestamp: new Date().toISOString(),
 *   }
 * })
 *
 * command("process-data", async ({ data }: { data: string[] }) => {
 *   const result = await processData(data)
 *   return { processed: result.length }
 * })
 * ```
 */
export function command<TParams = unknown, TResult = unknown>(
  name: string,
  handler: (params: TParams) => Promise<TResult>,
): void {
  commands.set(name, { name, handler: handler as (params: unknown) => Promise<unknown> })
}

/**
 * Get all registered commands.
 *
 * @internal
 */
export function getCommands(): Map<string, CommandDefinition> {
  return commands
}

/**
 * Execute a command by name.
 *
 * @param name - Command name to execute
 * @param params - Command parameters
 * @returns Command result
 *
 * @internal
 */
export async function runCommand(name: string, params: unknown): Promise<unknown> {
  const cmd = commands.get(name)

  if (!cmd) {
    throw new Error(`Unknown command: ${name}. Available: ${[...commands.keys()].join(", ")}`)
  }

  return await cmd.handler(params)
}
