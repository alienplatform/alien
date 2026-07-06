/**
 * WaitUntil drain coordination.
 *
 * The facade's {@link waitUntil} registrar (in `./registry.ts`) tracks promises
 * with no gRPC. This manager is the runtime half: it notifies the runtime when
 * a task is registered and, on drain, waits for every tracked task before
 * reporting completion.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type WaitUntilServiceClient as GeneratedClient,
  WaitUntilServiceDefinition,
} from "./generated/wait_until.js"
import { wrapGrpcCall } from "./grpc-utils.js"
import { getWaitUntilTasks, setOnTaskRegistered } from "./registry.js"

/**
 * WaitUntil manager for coordinating background tasks with the runtime.
 *
 * @internal
 */
export class WaitUntilManager {
  private readonly client: GeneratedClient
  private readonly applicationId: string

  constructor(channel: Channel, applicationId: string) {
    this.client = createClient(WaitUntilServiceDefinition, channel)
    this.applicationId = applicationId
  }

  /**
   * Install this manager as the registry's task-registered hook so each
   * `waitUntil` call notifies the runtime that a background task exists.
   */
  install(): void {
    setOnTaskRegistered(tracker => {
      this.notifyTaskRegistered().catch(error => {
        console.error("[alien:wait-until] notifyTaskRegistered failed:", error)
      })
      // Touch the tracker so the closure keeps a reference for debugging tools.
      void tracker.id
    })
  }

  /** Uninstall the hook (used during shutdown). */
  uninstall(): void {
    setOnTaskRegistered(undefined)
  }

  /**
   * Notify the runtime that a new background task was registered.
   */
  async notifyTaskRegistered(description?: string): Promise<void> {
    await wrapGrpcCall("WaitUntilService", "NotifyTaskRegistered", async () => {
      await this.client.notifyTaskRegistered({
        applicationId: this.applicationId,
        taskDescription: description,
      })
    })
  }

  /**
   * Wait for all tracked background tasks to complete.
   */
  async waitForAll(): Promise<void> {
    const pending = Array.from(getWaitUntilTasks().values())
      .filter(t => !t.completed)
      .map(t => t.promise.catch(() => {}))
    await Promise.all(pending)
  }

  /**
   * Get the current task count from the runtime.
   */
  async getTaskCount(): Promise<number> {
    return await wrapGrpcCall("WaitUntilService", "GetTaskCount", async () => {
      const response = await this.client.getTaskCount({ applicationId: this.applicationId })
      return response.taskCount
    })
  }

  /**
   * Wait for a drain signal from the runtime.
   */
  async waitForDrainSignal(): Promise<{ shouldDrain: boolean; drainReason: string }> {
    return await wrapGrpcCall("WaitUntilService", "WaitForDrainSignal", async () => {
      const response = await this.client.waitForDrainSignal({
        applicationId: this.applicationId,
        timeout: undefined,
      })
      return { shouldDrain: response.shouldDrain, drainReason: response.drainReason }
    })
  }

  /**
   * Notify the runtime that drain is complete.
   */
  async notifyDrainComplete(
    tasksDrained: number,
    success: boolean,
    errorMessage?: string,
  ): Promise<void> {
    await wrapGrpcCall("WaitUntilService", "NotifyDrainComplete", async () => {
      await this.client.notifyDrainComplete({
        applicationId: this.applicationId,
        tasksDrained,
        success,
        errorMessage,
      })
    })
  }
}
