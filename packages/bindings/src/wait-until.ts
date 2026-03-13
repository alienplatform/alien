/**
 * WaitUntil functionality for background task management.
 *
 * Allows functions to continue running after the response has been sent.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type WaitUntilServiceClient as GeneratedClient,
  WaitUntilServiceDefinition,
} from "./generated/wait_until.js"
import { wrapGrpcCall } from "./grpc-utils.js"

/**
 * Task tracker for managing background tasks.
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

// Internal storage for tracked tasks
const tasks: Map<string, TaskTracker> = new Map()
let taskIdCounter = 0

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
   * Register a background task.
   *
   * @param promise - The promise to track
   * @param description - Optional task description for debugging
   * @returns Task tracker
   */
  async register(promise: Promise<unknown>, description?: string): Promise<TaskTracker> {
    const id = `task-${++taskIdCounter}`
    const registeredAt = new Date()

    const tracker: TaskTracker = {
      id,
      promise,
      registeredAt,
      completed: false,
    }

    tasks.set(id, tracker)

    // Notify the runtime about the new task
    await wrapGrpcCall(
      "WaitUntilService",
      "NotifyTaskRegistered",
      async () => {
        await this.client.notifyTaskRegistered({
          applicationId: this.applicationId,
          taskDescription: description,
        })
      },
      {},
    )

    // Track completion
    promise
      .then(() => {
        tracker.completed = true
      })
      .catch(error => {
        tracker.completed = true
        tracker.error = error instanceof Error ? error : new Error(String(error))
      })

    return tracker
  }

  /**
   * Wait for all background tasks to complete.
   *
   * Called during drain to ensure all tasks finish before shutdown.
   */
  async waitForAll(): Promise<void> {
    const pending = Array.from(tasks.values())
      .filter(t => !t.completed)
      .map(t => t.promise.catch(() => {}))

    await Promise.all(pending)
  }

  /**
   * Get the current task count.
   */
  async getTaskCount(): Promise<number> {
    return await wrapGrpcCall(
      "WaitUntilService",
      "GetTaskCount",
      async () => {
        const response = await this.client.getTaskCount({
          applicationId: this.applicationId,
        })
        return response.taskCount
      },
      {},
    )
  }

  /**
   * Wait for a drain signal from the runtime.
   *
   * This blocks until the runtime signals that it's time to drain.
   */
  async waitForDrainSignal(): Promise<{
    shouldDrain: boolean
    drainReason: string
  }> {
    return await wrapGrpcCall(
      "WaitUntilService",
      "WaitForDrainSignal",
      async () => {
        const response = await this.client.waitForDrainSignal({
          applicationId: this.applicationId,
          timeout: undefined,
        })
        return {
          shouldDrain: response.shouldDrain,
          drainReason: response.drainReason,
        }
      },
      {},
    )
  }

  /**
   * Notify the runtime that drain is complete.
   *
   * @param tasksDrained - Number of tasks that were drained
   * @param success - Whether all tasks completed successfully
   * @param errorMessage - Optional error message if draining failed
   */
  async notifyDrainComplete(
    tasksDrained: number,
    success: boolean,
    errorMessage?: string,
  ): Promise<void> {
    await wrapGrpcCall(
      "WaitUntilService",
      "NotifyDrainComplete",
      async () => {
        await this.client.notifyDrainComplete({
          applicationId: this.applicationId,
          tasksDrained,
          success,
          errorMessage,
        })
      },
      {},
    )
  }

  /**
   * Get all tracked tasks.
   */
  getTasks(): Map<string, TaskTracker> {
    return tasks
  }

  /**
   * Clear all completed tasks.
   */
  clearCompleted(): void {
    for (const [id, tracker] of tasks) {
      if (tracker.completed) {
        tasks.delete(id)
      }
    }
  }
}

// Global wait-until manager instance
let globalManager: WaitUntilManager | undefined

/**
 * Initialize the global wait-until manager.
 *
 * @internal
 */
export function initWaitUntilManager(channel: Channel, applicationId: string): WaitUntilManager {
  globalManager = new WaitUntilManager(channel, applicationId)
  return globalManager
}

/**
 * Get the global wait-until manager.
 *
 * @internal
 */
export function getWaitUntilManager(): WaitUntilManager | undefined {
  return globalManager
}

/**
 * Register a background task to continue after the response.
 *
 * This allows your function to return a response while continuing
 * to do work in the background. The runtime will wait for all
 * registered tasks to complete before shutting down.
 *
 * @param promise - The promise to track
 *
 * @example
 * ```typescript
 * import { waitUntil } from "@alienplatform/bindings"
 *
 * export default {
 *   async fetch(request: Request): Promise<Response> {
 *     const data = await request.json()
 *
 *     // Start background work
 *     waitUntil(sendAnalytics(data))
 *     waitUntil(updateCache(data))
 *
 *     // Return immediately
 *     return Response.json({ status: "accepted" })
 *   }
 * }
 * ```
 */
export function waitUntil(promise: Promise<unknown>): void {
  if (!globalManager) {
    // If no manager, just track locally
    promise.catch(error => {
      console.error("Background task failed:", error)
    })
    return
  }

  globalManager.register(promise).catch(error => {
    console.error("Failed to register background task:", error)
  })
}
