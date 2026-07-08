/**
 * WaitUntil coordination.
 *
 * The facade's {@link waitUntil} registrar (in `./registry.ts`) tracks promises
 * with no gRPC. This manager is the runtime half: it notifies the runtime each
 * time a background task is registered.
 *
 * Graceful drain-on-shutdown (waiting for tracked tasks to settle before the
 * process exits) is a planned future feature; the runtime-side drain protocol
 * (WaitForDrainSignal/NotifyDrainComplete) exists in the generated client but
 * is not yet wired here.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type WaitUntilServiceClient as GeneratedClient,
  WaitUntilServiceDefinition,
} from "./generated/wait_until.js"
import { wrapGrpcCall } from "./grpc-utils.js"
import { setOnTaskRegistered } from "./registry.js"

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
    setOnTaskRegistered(() => {
      this.notifyTaskRegistered().catch(error => {
        console.error("[alien:wait-until] notifyTaskRegistered failed:", error)
      })
    })
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
}
