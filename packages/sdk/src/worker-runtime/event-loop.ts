/**
 * Worker event loop — connects to the runtime over the Worker protocol,
 * registers the handlers collected in {@link ./registry.ts}, and dispatches
 * incoming tasks (commands + storage/cron/queue events) to them.
 */

import type { StorageEvent, StorageEventType } from "@alienplatform/core"
import { type Channel, createClient } from "nice-grpc"
import { createGrpcChannel, getGrpcEndpoint } from "./channel.js"
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
import {
  type CronEvent,
  type QueueMessageEvent,
  getCommands,
  getEventHandlers,
  runCommand,
} from "./registry.js"

/**
 * Event loop runner for processing tasks from the runtime.
 *
 * @internal
 */
export class EventLoop {
  private readonly client: GeneratedClient
  private sendClient: GeneratedClient | undefined
  private readonly applicationId: string
  private running = false

  constructor(channel: Channel, applicationId: string) {
    this.client = createClient(ControlServiceDefinition, channel)
    this.applicationId = applicationId
  }

  /**
   * Get or create a separate gRPC client for sending task results.
   * Uses a dedicated channel to avoid HTTP/2 multiplexing issues with the
   * long-lived waitForTasks stream on some runtimes (e.g. Bun).
   */
  private async getSendClient(): Promise<GeneratedClient> {
    if (!this.sendClient) {
      const endpoint = getGrpcEndpoint()
      const sendChannel = await createGrpcChannel(endpoint)
      this.sendClient = createClient(ControlServiceDefinition, sendChannel)
    }
    return this.sendClient
  }

  /**
   * Register all handlers (events + commands) with the runtime.
   */
  async registerHandlers(): Promise<void> {
    const registrations: Promise<void>[] = []

    for (const { registration } of getEventHandlers().values()) {
      registrations.push(
        wrapGrpcCall("ControlService", "RegisterEventHandler", async () => {
          await this.client.registerEventHandler({
            handlerType: registration.type,
            resourceName: registration.source,
          })
        }),
      )
    }

    for (const command of getCommands().values()) {
      registrations.push(
        wrapGrpcCall("ControlService", "RegisterEventHandler", async () => {
          await this.client.registerEventHandler({
            handlerType: "command",
            resourceName: command.name,
          })
        }),
      )
    }

    await Promise.all(registrations)
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

  private async processTasks(): Promise<void> {
    const stream = this.client.waitForTasks({ applicationId: this.applicationId })
    for await (const task of stream) {
      try {
        await this.handleTask(task)
      } catch (error) {
        console.error(
          `[alien:event-loop] handleTask threw (task will not break stream): id=${task.taskId} error=${error instanceof Error ? error.message : String(error)}`,
        )
      }
    }
  }

  private async handleTask(task: Task): Promise<void> {
    try {
      if (task.arcCommand) {
        const result = await this.handleCommand(task.arcCommand)
        await this.sendTaskResult(task.taskId, { success: true, data: result })
        return
      }

      let matchedEntry: { handler: (event: unknown) => Promise<void> } | undefined
      for (const entry of getEventHandlers().values()) {
        const src = entry.registration.source
        if (task.storageEvent && entry.registration.type === "storage") {
          if (src === "*" || src === task.storageEvent.bucket) {
            matchedEntry = entry
            break
          }
        } else if (task.cronEvent && entry.registration.type === "cron") {
          if (src === "*" || src === task.cronEvent.scheduleName) {
            matchedEntry = entry
            break
          }
        } else if (task.queueMessage && entry.registration.type === "queue") {
          if (src === "*" || src === task.queueMessage.source) {
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
        await matchedEntry.handler(this.fromProtoStorageEvent(task.storageEvent))
      } else if (task.cronEvent) {
        await matchedEntry.handler(this.fromProtoCronEvent(task.cronEvent))
      } else if (task.queueMessage) {
        await matchedEntry.handler(this.fromProtoQueueMessage(task.queueMessage))
      }

      await this.sendTaskResult(task.taskId, { success: true })
    } catch (error) {
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
    let params: unknown = {}
    if (command.params && command.params.length > 0) {
      try {
        params = JSON.parse(new TextDecoder().decode(command.params))
      } catch (error) {
        // Fail fast: params that don't decode mean a malformed command, not an
        // empty one. Surface it as a task error (mirroring the pull receiver,
        // which submits the decode failure rather than running the handler)
        // instead of silently invoking the handler with `{}`. `handleTask`
        // catches this and reports it as a failed task result.
        throw new Error(
          `Command '${command.commandName}' has malformed JSON params: ${
            error instanceof Error ? error.message : String(error)
          }`,
        )
      }
    }
    return await runCommand(command.commandName, params)
  }

  private async sendTaskResult(
    taskId: string,
    result: { success: boolean; error?: string; data?: unknown },
  ): Promise<void> {
    const client = await this.getSendClient()
    const signal = AbortSignal.timeout(30_000)
    await wrapGrpcCall("ControlService", "SendTaskResult", async () => {
      if (result.success) {
        const responseData = result.data
          ? new TextEncoder().encode(JSON.stringify(result.data))
          : new Uint8Array()
        await client.sendTaskResult({ taskId, success: { responseData } }, { signal })
      } else {
        await client.sendTaskResult(
          { taskId, error: { code: "ERROR", message: result.error ?? "Unknown error" } },
          { signal },
        )
      }
    })
  }

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
      eventType: eventTypeMap[proto.eventType] ?? "unknown",
      bucketName: proto.bucket,
      objectKey: proto.key,
      timestamp: proto.timestamp?.toISOString() ?? new Date().toISOString(),
      size: proto.size ? Number(proto.size) : undefined,
      contentType: proto.contentType || undefined,
      etag: proto.etag || undefined,
      metadata:
        proto.metadata && Object.keys(proto.metadata).length > 0 ? proto.metadata : undefined,
      copySource: undefined,
      previousTier: undefined,
      currentTier: proto.currentTier || undefined,
      region: proto.region || undefined,
      versionId: proto.versionId || undefined,
    }
  }

  private fromProtoCronEvent(proto: ProtoCronEvent): CronEvent {
    return {
      scheduleName: proto.scheduleName,
      timestamp: proto.scheduledTime?.toISOString() ?? new Date().toISOString(),
    }
  }

  private fromProtoQueueMessage<T>(proto: ProtoQueueMessage): QueueMessageEvent<T> {
    let payload: unknown = null
    if (proto.payload && proto.payload.length > 0) {
      try {
        payload = JSON.parse(new TextDecoder().decode(proto.payload))
      } catch {
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
      attributes:
        proto.attributes && Object.keys(proto.attributes).length > 0 ? proto.attributes : undefined,
    }
  }
}
