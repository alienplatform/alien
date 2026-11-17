import { z } from "zod/v4"
import { AlienError, defineError } from "./error.js"
import {
  type Queue as QueueConfig,
  type QueueDeadLetter,
  QueueSchema,
  type ResourceType,
} from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type { Queue as QueueConfig, QueueDeadLetter, QueueOutputs } from "./generated/index.js"
export { QueueSchema as QueueConfigSchema } from "./generated/index.js"

/** Options for a queue's dead-letter setting. */
export type QueueDeadLetterOptions = Partial<Pick<QueueDeadLetter, "maxAttempts">>

const InvalidQueueDeadLetter = defineError({
  code: "INVALID_QUEUE_DEAD_LETTER",
  context: z.object({ reason: z.string() }),
  message: ({ reason }) => `Invalid queue dead-letter setting: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/** Deliveries after which a message moves to the dead-letter queue, unless set. */
const DEFAULT_DEAD_LETTER_MAX_ATTEMPTS = 5

/**
 * Represents a message queue resource with minimal, portable semantics.
 * Queue integrates with platform-native services (AWS SQS, GCP Pub/Sub, Azure Service Bus).
 */
export class Queue extends ResourceBuilder {
  private _config: Partial<QueueConfig> = {}

  /**
   * Creates a new Queue builder.
   * @param id Identifier for the queue. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    super()
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any queue resource.
   * Used for creating permission targets that apply to all queue resources.
   * @returns The "queue" resource type.
   */
  public static any(): ResourceType {
    return "queue"
  }

  /**
   * Moves a message to another Queue in the same stack after `maxAttempts` failed deliveries
   * (5 to 100, default 5). The target is a normal queue: link it, receive from it, or trigger a
   * Worker from it. Both queues must be Frozen or both Live, and the target cannot have a
   * dead-letter queue itself.
   *
   * The count is approximate: it is the cloud's own delivery count (SQS counts every receive,
   * Pub/Sub is best effort), and a batch `retry` counts as a failed delivery. The payload is
   * kept; there is no portable failure reason.
   * @param queue The Queue that receives the messages.
   * @param options Optional `maxAttempts`.
   * @returns The Queue builder instance.
   */
  public deadLetter(queue: Resource, options: QueueDeadLetterOptions = {}): this {
    if (queue.config.type !== "queue") {
      throw new AlienError(
        InvalidQueueDeadLetter.create({
          reason: `expected a Queue, received '${queue.config.type}'`,
        }),
      )
    }
    this._config.deadLetter = {
      queue: queue.ref(),
      maxAttempts: options.maxAttempts ?? DEFAULT_DEAD_LETTER_MAX_ATTEMPTS,
    }
    return this
  }

  /**
   * Builds and validates the queue configuration.
   * @returns An immutable Resource representing the configured queue.
   * @throws Error if the queue configuration is invalid.
   */
  public build(): Resource {
    const config = QueueSchema.parse(this._config)

    return this.resource({
      type: "queue",
      ...config,
    })
  }
}
