/**
 * Event handling for Alien bindings.
 *
 * Provides handlers for storage events, cron/scheduled events, and queue messages.
 */

// Import canonical event types from core
import type {
  QueueMessage,
  ScheduledEvent,
  StorageEvent,
  StorageEventType,
} from "@alienplatform/core"
import { type Channel, createClient } from "nice-grpc"
import { getCommands, runCommand } from "./commands.js"
import {
  ControlServiceDefinition,
  type ControlServiceClient as GeneratedClient,
  type ArcCommand as ProtoArcCommand,
  type CronEvent as ProtoCronEvent,
  type QueueMessage as ProtoQueueMessage,
  type StorageEvent as ProtoStorageEvent,
  type Task,
} from "./generated/control.js"
import { wrapGrpcCall } from "./grpc-utils.js"

// Re-export core types for convenience
export type { StorageEvent, StorageEventType, ScheduledEvent, QueueMessage }

/**
 * Cron/scheduled event with schedule info.
 * Extends ScheduledEvent with the schedule name.
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

// Internal storage for event handlers
const eventHandlers: Map<
  string,
  {
    registration: EventRegistration
    handler: (event: unknown) => Promise<void>
  }
> = new Map()

let handlerIdCounter = 0

/**
 * Register a storage event handler.
 *
 * @param bucket - Bucket name to listen for events
 * @param handler - Handler function for storage events
 * @param options - Handler options
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onStorageEvent } from "@alienplatform/bindings"
 *
 * onStorageEvent("uploads", async (event) => {
 *   console.log("File uploaded:", event.objectKey)
 *   await processFile(event.bucketName, event.objectKey)
 * })
 * ```
 */
export function onStorageEvent(
  bucket: string,
  handler: (event: StorageEvent) => Promise<void>,
  options?: { prefix?: string },
): () => void {
  const id = `storage-${++handlerIdCounter}`

  eventHandlers.set(id, {
    registration: {
      id,
      type: "storage",
      source: bucket,
      prefix: options?.prefix,
    },
    handler: handler as (event: unknown) => Promise<void>,
  })

  return () => {
    eventHandlers.delete(id)
  }
}

/**
 * Register a cron/scheduled event handler.
 *
 * @param scheduleName - Schedule name to listen for
 * @param handler - Handler function for scheduled events
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onCronEvent } from "@alienplatform/bindings"
 *
 * onCronEvent("hourly-task", async (event) => {
 *   console.log("Scheduled run at:", event.timestamp)
 *   await runHourlyTask()
 * })
 * ```
 */
export function onCronEvent(
  scheduleName: string,
  handler: (event: CronEvent) => Promise<void>,
): () => void {
  const id = `cron-${++handlerIdCounter}`

  eventHandlers.set(id, {
    registration: {
      id,
      type: "cron",
      source: scheduleName,
    },
    handler: handler as (event: unknown) => Promise<void>,
  })

  return () => {
    eventHandlers.delete(id)
  }
}

/**
 * Register a queue message handler.
 *
 * @param queueName - Queue name to listen for messages
 * @param handler - Handler function for queue messages
 * @returns Unsubscribe function
 *
 * @example
 * ```typescript
 * import { onQueueMessage } from "@alienplatform/bindings"
 *
 * onQueueMessage("tasks", async (message) => {
 *   console.log("Processing task:", message.payload)
 *   await processTask(message.payload)
 * })
 * ```
 */
export function onQueueMessage<T = unknown>(
  queueName: string,
  handler: (message: QueueMessageEvent<T>) => Promise<void>,
): () => void {
  const id = `queue-${++handlerIdCounter}`

  eventHandlers.set(id, {
    registration: {
      id,
      type: "queue",
      source: queueName,
    },
    handler: handler as (event: unknown) => Promise<void>,
  })

  return () => {
    eventHandlers.delete(id)
  }
}

/**
 * Get all registered event handlers.
 *
 * @internal
 */
export function getEventHandlers(): Map<
  string,
  {
    registration: EventRegistration
    handler: (event: unknown) => Promise<void>
  }
> {
  return eventHandlers
}

/**
 * Event loop runner for processing tasks from the control plane.
 *
 * @internal
 */
export class EventLoop {
  private readonly client: GeneratedClient
  private readonly applicationId: string
  private running = false

  constructor(channel: Channel, applicationId: string) {
    this.client = createClient(ControlServiceDefinition, channel)
    this.applicationId = applicationId
  }

  /**
   * Register all event handlers with the control plane.
   */
  async registerHandlers(): Promise<void> {
    // Register event handlers
    for (const { registration } of eventHandlers.values()) {
      await wrapGrpcCall(
        "ControlService",
        "RegisterEventHandler",
        async () => {
          await this.client.registerEventHandler({
            handlerType: registration.type,
            resourceName: registration.source,
          })
        },
        {},
      )
    }

    // Register command handlers
    for (const command of getCommands().values()) {
      await wrapGrpcCall(
        "ControlService",
        "RegisterEventHandler",
        async () => {
          await this.client.registerEventHandler({
            handlerType: "command",
            resourceName: command.name,
          })
        },
        {},
      )
    }
  }

  /**
   * Start the event loop.
   */
  async start(): Promise<void> {
    this.running = true

    while (this.running) {
      try {
        await this.processTasks()
      } catch (error) {
        console.error("[alien:event-loop] processTasks threw:", error)
        await new Promise(resolve => setTimeout(resolve, 1000))
      }
    }
  }

  /**
   * Stop the event loop.
   */
  stop(): void {
    this.running = false
  }

  private async processTasks(): Promise<void> {
    console.log(`[alien:event-loop] Opening waitForTasks stream`)
    const stream = this.client.waitForTasks({
      applicationId: this.applicationId,
    })

    for await (const task of stream) {
      try {
        await this.handleTask(task)
      } catch (error) {
        // Catch errors here so the for-await loop continues processing tasks
        // instead of breaking and reconnecting (which could miss buffered tasks).
        console.error(
          `[alien:event-loop] handleTask threw (task will not break stream): id=${task.taskId} error=${error instanceof Error ? error.message : String(error)}`,
        )
      }
    }
    console.log(`[alien:event-loop] waitForTasks stream ended`)
  }

  private async handleTask(task: Task): Promise<void> {
    try {
      // Handle commands
      if (task.arcCommand) {
        console.log(
          `[alien:event-loop] Received command task: id=${task.taskId} command=${task.arcCommand.commandName}`,
        )
        const result = await this.handleCommand(task.arcCommand)
        const responseData = result
          ? new TextEncoder().encode(JSON.stringify(result))
          : new Uint8Array()
        console.log(
          `[alien:event-loop] Command handler completed: id=${task.taskId} responseSize=${responseData.length}`,
        )
        await this.sendTaskResult(task.taskId, { success: true, data: result })
        console.log(
          `[alien:event-loop] sendTaskResult completed: id=${task.taskId}`,
        )
        return
      }

      // Handle events (storage, cron, queue)
      let matchedEntry: { handler: (event: unknown) => Promise<void> } | undefined

      for (const entry of eventHandlers.values()) {
        if (task.storageEvent && entry.registration.type === "storage") {
          if (entry.registration.source === task.storageEvent.bucket) {
            matchedEntry = entry
            break
          }
        } else if (task.cronEvent && entry.registration.type === "cron") {
          if (entry.registration.source === task.cronEvent.scheduleName) {
            matchedEntry = entry
            break
          }
        } else if (task.queueMessage && entry.registration.type === "queue") {
          if (entry.registration.source === task.queueMessage.source) {
            matchedEntry = entry
            break
          }
        }
      }

      if (!matchedEntry) {
        console.warn(`No handler found for task: ${task.taskId}`)
        return
      }

      if (task.storageEvent) {
        const storageEvent = this.fromProtoStorageEvent(task.storageEvent)
        await matchedEntry.handler(storageEvent)
      } else if (task.cronEvent) {
        const cronEvent = this.fromProtoCronEvent(task.cronEvent)
        await matchedEntry.handler(cronEvent)
      } else if (task.queueMessage) {
        const queueEvent = this.fromProtoQueueMessage(task.queueMessage)
        await matchedEntry.handler(queueEvent)
      }

      // Report success
      await this.sendTaskResult(task.taskId, { success: true })
    } catch (error) {
      // Report error
      console.error(
        `[alien:event-loop] Task error: id=${task.taskId} error=${error instanceof Error ? error.message : String(error)}`,
      )
      try {
        await this.sendTaskResult(task.taskId, {
          success: false,
          error: error instanceof Error ? error.message : String(error),
        })
      } catch (sendError) {
        console.error(
          `[alien:event-loop] Failed to send error result: id=${task.taskId} sendError=${sendError instanceof Error ? sendError.message : String(sendError)}`,
        )
      }
    }
  }

  private async handleCommand(command: ProtoArcCommand): Promise<unknown> {
    // Parse command parameters
    let params: unknown = {}
    if (command.params && command.params.length > 0) {
      try {
        const text = new TextDecoder().decode(command.params)
        params = JSON.parse(text)
      } catch {
        // If not valid JSON, use empty object
        params = {}
      }
    }

    // Run the command
    return await runCommand(command.commandName, params)
  }

  private async sendTaskResult(
    taskId: string,
    result: { success: boolean; error?: string; data?: unknown },
  ): Promise<void> {
    // Use a 30-second timeout to prevent hanging if the gRPC response is delayed
    const signal = AbortSignal.timeout(30_000)
    await wrapGrpcCall(
      "ControlService",
      "SendTaskResult",
      async () => {
        if (result.success) {
          const responseData = result.data
            ? new TextEncoder().encode(JSON.stringify(result.data))
            : new Uint8Array()
          await this.client.sendTaskResult(
            {
              taskId,
              success: { responseData },
            },
            { signal },
          )
        } else {
          await this.client.sendTaskResult(
            {
              taskId,
              error: { code: "ERROR", message: result.error ?? "Unknown error" },
            },
            { signal },
          )
        }
      },
      {},
    )
  }

  /**
   * Transform proto StorageEvent to core StorageEvent type.
   */
  private fromProtoStorageEvent(proto: ProtoStorageEvent): StorageEvent {
    const eventTypeMap: Record<string, StorageEventType> = {
      created: "created",
      deleted: "deleted",
      copied: "copied",
      metadata_updated: "metadataUpdated",
      restored: "restored",
      tier_changed: "tierChanged",
    }

    return {
      eventType: eventTypeMap[proto.eventType] ?? ("unknown" as StorageEventType),
      bucketName: proto.bucket,
      objectKey: proto.key,
      timestamp: proto.timestamp?.toISOString() ?? new Date().toISOString(),
      size: proto.size ? Number(proto.size) : undefined,
      contentType: proto.contentType || undefined,
      // These fields are not in the proto but are in the core type
      etag: undefined,
      metadata: undefined,
      copySource: undefined,
      previousTier: undefined,
      currentTier: undefined,
      region: undefined,
      versionId: undefined,
    }
  }

  /**
   * Transform proto CronEvent to CronEvent type.
   */
  private fromProtoCronEvent(proto: ProtoCronEvent): CronEvent {
    return {
      scheduleName: proto.scheduleName,
      timestamp: proto.scheduledTime?.toISOString() ?? new Date().toISOString(),
    }
  }

  /**
   * Transform proto QueueMessage to QueueMessageEvent type.
   */
  private fromProtoQueueMessage<T>(proto: ProtoQueueMessage): QueueMessageEvent<T> {
    // Proto payload is Uint8Array (JSON bytes), parse it
    let payload: unknown = null
    if (proto.payload && proto.payload.length > 0) {
      try {
        const text = new TextDecoder().decode(proto.payload)
        payload = JSON.parse(text)
      } catch {
        // If not valid JSON, use raw bytes
        payload = proto.payload
      }
    }

    return {
      id: proto.id,
      source: proto.source,
      receiptHandle: proto.receiptHandle,
      payload: payload as T,
      attemptCount: proto.attemptCount,
      timestamp: proto.timestamp ?? new Date(),
    }
  }
}
