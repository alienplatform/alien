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
 * Per-invocation metadata passed to a Worker command handler alongside its
 * params. Mirrors the pull receiver's context: `attempt > 1` means
 * redelivery (at-least-once semantics), and `deadline` bounds execution.
 */
export interface WorkerCommandContext {
  /** Unique command identifier. */
  commandId: string
  /** Delivery attempt, starting at 1. */
  attempt: number
  /** Deadline for completion, when the sender set one. */
  deadline?: Date
}

/** The part of Standard Schema v1 used to validate command inputs. */
export interface StandardSchema<Input = unknown, Output = Input> {
  readonly "~standard": {
    readonly version: 1
    readonly vendor: string
    readonly validate: (
      value: unknown,
    ) =>
      | { readonly value: Output; readonly issues?: undefined }
      | { readonly issues: ReadonlyArray<{ readonly message: string }> }
      | Promise<
          | { readonly value: Output; readonly issues?: undefined }
          | { readonly issues: ReadonlyArray<{ readonly message: string }> }
        >
    readonly types?: { readonly input: Input; readonly output: Output }
  }
}

/** Infer the validated output type of a Standard Schema. */
export type StandardSchemaOutput<Schema extends StandardSchema> = NonNullable<
  Schema["~standard"]["types"]
>["output"]

/**
 * Command definition.
 */
export interface CommandDefinition {
  /** Command name */
  name: string
  /** Handler function that receives params and returns a result */
  handler: (params: unknown, context: WorkerCommandContext) => Promise<unknown>
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
 * The handler receives the decoded params and, optionally, a
 * {@link WorkerCommandContext} with `commandId`, `attempt`, and `deadline` —
 * use `attempt` to make redeliveries idempotent (at-least-once semantics).
 *
 * @param name - Command name
 * @param handler - Handler that receives params (and optionally the context)
 *
 * @example
 * ```typescript
 * import { command } from "@alienplatform/sdk"
 *
 * command("echo", schema, async ({ message }, { attempt }) => {
 *   return { message, attempt }
 * })
 * ```
 */
export function command<TResult = unknown>(
  name: string,
  handler: (params: unknown, context: WorkerCommandContext) => TResult | Promise<TResult>,
): void
export function command<Schema extends StandardSchema, TResult = unknown>(
  name: string,
  schema: Schema,
  handler: (
    params: StandardSchemaOutput<Schema>,
    context: WorkerCommandContext,
  ) => TResult | Promise<TResult>,
): void
export function command<Schema extends StandardSchema, TResult = unknown>(
  name: string,
  schemaOrHandler:
    | Schema
    | ((params: unknown, context: WorkerCommandContext) => TResult | Promise<TResult>),
  validatedHandler?: (
    params: StandardSchemaOutput<Schema>,
    context: WorkerCommandContext,
  ) => TResult | Promise<TResult>,
): void {
  const schema = validatedHandler === undefined ? undefined : (schemaOrHandler as Schema)
  const handler = (validatedHandler ?? schemaOrHandler) as (
    params: unknown,
    context: WorkerCommandContext,
  ) => TResult | Promise<TResult>
  registry().commands.set(name, {
    name,
    handler: async (params, context) => {
      if (schema === undefined) return await handler(params, context)
      const result = await schema["~standard"].validate(params)
      if (result.issues !== undefined) {
        const details = result.issues.map(issue => issue.message).join("; ")
        throw new Error(`Command input failed validation${details ? `: ${details}` : ""}`)
      }
      return await handler(result.value, context)
    },
  })
}

/** Get all registered commands. @internal */
export function getCommands(): Map<string, CommandDefinition> {
  return registry().commands
}

/** Execute a registered command by name. @internal */
export async function runCommand(
  name: string,
  params: unknown,
  context: WorkerCommandContext,
): Promise<unknown> {
  const commands = registry().commands
  const cmd = commands.get(name)
  if (!cmd) {
    throw new Error(`Unknown command: ${name}. Available: ${[...commands.keys()].join(", ")}`)
  }
  return await cmd.handler(params, context)
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
    // Drop the tracker once it settles so the map holds only in-flight tasks and
    // does not grow without bound over a long-lived worker's lifetime.
    .finally(() => {
      state.waitUntilTasks.delete(id)
    })

  try {
    state.onTaskRegistered?.(tracker)
  } catch (error) {
    console.error("[alien:wait-until] onTaskRegistered hook failed:", error)
  }
}

/** Install the runtime's task-registered hook. @internal */
export function setOnTaskRegistered(hook: ((tracker: TaskTracker) => void) | undefined): void {
  registry().onTaskRegistered = hook
}
