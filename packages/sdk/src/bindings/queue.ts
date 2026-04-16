/**
 * Queue binding implementation.
 *
 * Provides message queue operations for async task processing.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type QueueServiceClient as GeneratedQueueServiceClient,
  type MessagePayload,
  type QueueMessage as QueueMessageProto,
  QueueServiceDefinition,
} from "../generated/queue.js"
import { wrapGrpcCall } from "../grpc-utils.js"
import type { ReceivedQueueMessage } from "../types.js"

/**
 * Queue binding for message queue operations.
 *
 * @example
 * ```typescript
 * import { queue } from "@alienplatform/sdk"
 *
 * const tasks = queue("task-queue")
 *
 * // Send a message (JSON)
 * await tasks.send("process-image", { imageId: "123", format: "webp" })
 *
 * // Receive messages
 * const messages = await tasks.receive("process-image", 10)
 * for (const msg of messages) {
 *   await processImage(msg.payload)
 *   await tasks.ack("process-image", msg.receiptHandle)
 * }
 * ```
 */
export class Queue {
  private readonly client: GeneratedQueueServiceClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(QueueServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Send a message to a queue.
   *
   * @param queueName - Name of the queue
   * @param payload - Message payload (object for JSON, string for text)
   */
  async send(queueName: string, payload: unknown): Promise<void> {
    const message = this.toProtoPayload(payload)

    await wrapGrpcCall(
      "QueueService",
      "Send",
      async () => {
        await this.client.send({
          bindingName: this.bindingName,
          queue: queueName,
          message,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Send a text message to a queue.
   *
   * @param queueName - Name of the queue
   * @param text - Text message
   */
  async sendText(queueName: string, text: string): Promise<void> {
    await wrapGrpcCall(
      "QueueService",
      "Send",
      async () => {
        await this.client.send({
          bindingName: this.bindingName,
          queue: queueName,
          message: { text },
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Receive messages from a queue.
   *
   * @param queueName - Name of the queue
   * @param maxMessages - Maximum number of messages to receive (1-10)
   * @returns Array of messages with receipt handles
   */
  async receive<T = unknown>(
    queueName: string,
    maxMessages = 1,
  ): Promise<ReceivedQueueMessage<T>[]> {
    return await wrapGrpcCall(
      "QueueService",
      "Receive",
      async () => {
        const response = await this.client.receive({
          bindingName: this.bindingName,
          queue: queueName,
          maxMessages: Math.min(Math.max(1, maxMessages), 10),
        })
        return response.messages.map(msg => this.fromProtoMessage<T>(msg))
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Acknowledge a message (remove it from the queue).
   *
   * @param queueName - Name of the queue
   * @param receiptHandle - Receipt handle from the received message
   */
  async ack(queueName: string, receiptHandle: string): Promise<void> {
    await wrapGrpcCall(
      "QueueService",
      "Ack",
      async () => {
        await this.client.ack({
          bindingName: this.bindingName,
          queue: queueName,
          receiptHandle,
        })
      },
      { bindingName: this.bindingName },
    )
  }

  /**
   * Process messages from a queue with automatic acknowledgment.
   *
   * @param queueName - Name of the queue
   * @param handler - Handler function for each message
   * @param options - Processing options
   *
   * @example
   * ```typescript
   * await tasks.process("image-tasks", async (payload) => {
   *   await processImage(payload.imageId)
   * })
   * ```
   */
  async process<T = unknown>(
    queueName: string,
    handler: (payload: T) => Promise<void>,
    options?: { maxMessages?: number },
  ): Promise<void> {
    const maxMessages = options?.maxMessages ?? 10

    const messages = await this.receive<T>(queueName, maxMessages)

    for (const message of messages) {
      await handler(message.payload)
      await this.ack(queueName, message.receiptHandle)
    }
  }

  // Private helpers

  private toProtoPayload(payload: unknown): MessagePayload {
    if (typeof payload === "string") {
      return { text: payload }
    }
    return { json: JSON.stringify(payload) }
  }

  private fromProtoMessage<T>(proto: QueueMessageProto): ReceivedQueueMessage<T> {
    let payload: unknown

    if (proto.payload?.json) {
      payload = JSON.parse(proto.payload.json)
    } else if (proto.payload?.text) {
      payload = proto.payload.text
    } else {
      payload = null
    }

    return {
      payload: payload as T,
      receiptHandle: proto.receiptHandle,
    }
  }
}
