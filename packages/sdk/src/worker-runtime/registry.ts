/**
 * Worker handler registries — the facade's protocol-only surface.
 *
 * `command`, `onStorageEvent`, `onCronEvent`, `onQueueMessage`, and `waitUntil`
 * register into these in-memory maps; {@link runWorker} (in `./index.ts`) reads
 * them to register handlers with the runtime and dispatch tasks. There is no
 * gRPC here — this module is what the facade root re-exports, so the facade
 * bundle stays free of Worker-protocol dependencies.
 *
 * The `@alienplatform/sdk` root (`dist/index.js`) and the
 * `@alienplatform/sdk/worker-runtime` subpath (`dist/worker-runtime/index.js`)
 * are separate entry bundles. A user's app registers handlers by importing the
 * root; the generated bootstrap drives them by importing the subpath. For those
 * two bundles to observe the SAME registration state, the maps are stored on
 * `globalThis` under a shared `Symbol.for` key rather than as ordinary
 * module-level state (which would be duplicated per bundle).
 */

import type {
  QueueMessage,
  ScheduledEvent,
  StorageEvent,
  StorageEventType,
} from "@alienplatform/core"

// Re-export the canonical core event types for the facade to re-export.
export type { StorageEvent, StorageEventType, ScheduledEvent, QueueMessage }

/**
 * Cron/scheduled event with schedule info.
 */
export interface CronEvent extends ScheduledEvent {
  /** Schedule name */
  scheduleName: string
}

/**
 * Queue message event with queue context.
 */
export interface QueueMessageEvent<T = unknown> {
  /** Message ID */
  id: string
  /** Source queue name */
  source: string
  /** Receipt handle for acknowledgment */
  receiptHandle: string
  /** Message payload */
  payload: T
  /** Delivery attempt count */
  attemptCount: number
  /** Message timestamp */
  timestamp: Date
  /** Platform-specific message attributes */
  attributes?: { [key: string]: string }
}

/**
 * Command definition.
 */
export interface CommandDefinition {
  /** Command name */
  name: string
  /** Handler function that receives params and returns a result */
  handler: (params: unknown) => Promise<unknown>
}

/**
 * Event handler registration.
 */
export interface EventRegistration {
  /** Handler ID */
  id: string
  /** Event type */
  type: "storage" | "cron" | "queue"
  /** Source filter (bucket name, queue name, etc.) */
  source: string
  /** Optional prefix filter (for storage events) */
  prefix?: string
}

/** A single registered event handler. */
export interface EventHandlerEntry {
  registration: EventRegistration
  handler: (event: unknown) => Promise<void>
}

/**
 * A tracked `waitUntil` background task.
 */
export interface TaskTracker {
  /** Unique task ID */
  id: string
  /** Task promise */
  promise: Promise<unknown>
  /** When the task was registered */
  registeredAt: Date
  /** Whether the task has completed */
  completed: boolean
  /** Error if the task failed */
  error?: Error
}

interface RegistryState {
  commands: Map<string, CommandDefinition>
  eventHandlers: Map<string, EventHandlerEntry>
  waitUntilTasks: Map<string, TaskTracker>
  counters: { handler: number; task: number }
  /**
   * Hook installed by the Worker runtime so a `waitUntil` call can notify the
   * runtime that a background task exists. Undefined outside a running Worker
   * (e.g. unit tests), where tasks are tracked locally only.
   */
  onTaskRegistered?: (tracker: TaskTracker) => void
}

const REGISTRY_KEY = Symbol.for("@alienplatform/sdk#worker-runtime-registry")

function registry(): RegistryState {
  const holder = globalThis as { [REGISTRY_KEY]?: RegistryState }
  let state = holder[REGISTRY_KEY]
  if (!state) {
    state = {
      commands: new Map(),
      eventHandlers: new Map(),
      waitUntilTasks: new Map(),
      counters: { handler: 0, task: 0 },
    }
    holder[REGISTRY_KEY] = state
  }
  return state
}

// ============================================================================
// Commands
// ============================================================================

/**
 * Register a command handler.
 *
 * @param name - Command name
 * @param handler - Handler that receives params and returns a result
 *
 * @example
 * ```typescript
 * import { command } from "@alienplatform/sdk"
 *
 * command("echo", async ({ message }: { message: string }) => {
 *   return { message, timestamp: new Date().toISOString() }
 * })
 * ```
 */
export function command<TParams = unknown, TResult = unknown>(
  name: string,
  handler: (params: TParams) => Promise<TResult>,
): void {
  registry().commands.set(name, {
    name,
    handler: handler as (params: unknown) => Promise<unknown>,
  })
}

/** Get all registered commands. @internal */
export function getCommands(): Map<string, CommandDefinition> {
  return registry().commands
}

/** Execute a registered command by name. @internal */
export async function runCommand(name: string, params: unknown): Promise<unknown> {
  const commands = registry().commands
  const cmd = commands.get(name)
  if (!cmd) {
    throw new Error(`Unknown command: ${name}. Available: ${[...commands.keys()].join(", ")}`)
  }
  return await cmd.handler(params)
}

// ============================================================================
// Event handlers
// ============================================================================

/**
 * Register a storage event handler.
 *
 * @param bucket - Bucket name (or `"*"` for all buckets)
 * @param handler - Handler function for storage events
 * @param options - Handler options
 * @returns Unsubscribe function
 */
export function onStorageEvent(
  bucket: string,
  handler: (event: StorageEvent) => Promise<void>,
  options?: { prefix?: string },
): () => void {
  const state = registry()
  const id = `storage-${++state.counters.handler}`
  state.eventHandlers.set(id, {
    registration: { id, type: "storage", source: bucket, prefix: options?.prefix },
    handler: handler as (event: unknown) => Promise<void>,
  })
  return () => {
    state.eventHandlers.delete(id)
  }
}

/**
 * Register a cron/scheduled event handler.
 *
 * @param scheduleName - Schedule name (or `"*"` for all schedules)
 * @param handler - Handler function for scheduled events
 * @returns Unsubscribe function
 */
export function onCronEvent(
  scheduleName: string,
  handler: (event: CronEvent) => Promise<void>,
): () => void {
  const state = registry()
  const id = `cron-${++state.counters.handler}`
  state.eventHandlers.set(id, {
    registration: { id, type: "cron", source: scheduleName },
    handler: handler as (event: unknown) => Promise<void>,
  })
  return () => {
    state.eventHandlers.delete(id)
  }
}

/**
 * Register a queue message handler.
 *
 * @param queueName - Queue name (or `"*"` for all queues)
 * @param handler - Handler function for queue messages
 * @returns Unsubscribe function
 */
export function onQueueMessage<T = unknown>(
  queueName: string,
  handler: (message: QueueMessageEvent<T>) => Promise<void>,
): () => void {
  const state = registry()
  const id = `queue-${++state.counters.handler}`
  state.eventHandlers.set(id, {
    registration: { id, type: "queue", source: queueName },
    handler: handler as (event: unknown) => Promise<void>,
  })
  return () => {
    state.eventHandlers.delete(id)
  }
}

/** Get all registered event handlers. @internal */
export function getEventHandlers(): Map<string, EventHandlerEntry> {
  return registry().eventHandlers
}

// ============================================================================
// WaitUntil
// ============================================================================

/**
 * Register a background task to continue after the response.
 *
 * The runtime waits for all registered tasks to complete before shutting down.
 * This is the protocol-only registrar: it tracks the promise and (inside a
 * running Worker) notifies the runtime via the installed hook. The gRPC drain
 * coordination lives in {@link runWorker}.
 *
 * @param promise - The promise to track
 */
export function waitUntil(promise: Promise<unknown>): void {
  const state = registry()
  const id = `task-${++state.counters.task}`
  const tracker: TaskTracker = {
    id,
    promise,
    registeredAt: new Date(),
    completed: false,
  }
  state.waitUntilTasks.set(id, tracker)

  promise
    .then(() => {
      tracker.completed = true
    })
    .catch(error => {
      tracker.completed = true
      tracker.error = error instanceof Error ? error : new Error(String(error))
    })

  try {
    state.onTaskRegistered?.(tracker)
  } catch (error) {
    console.error("[alien:wait-until] onTaskRegistered hook failed:", error)
  }
}

/** Get all tracked `waitUntil` tasks. @internal */
export function getWaitUntilTasks(): Map<string, TaskTracker> {
  return registry().waitUntilTasks
}

/** Install the runtime's task-registered hook. @internal */
export function setOnTaskRegistered(hook: ((tracker: TaskTracker) => void) | undefined): void {
  registry().onTaskRegistered = hook
}
