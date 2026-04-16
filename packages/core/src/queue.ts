import { type Queue as QueueConfig, QueueSchema, type ResourceType } from "./generated/index.js"
import { Resource } from "./resource.js"

export type { QueueOutputs, Queue as QueueConfig } from "./generated/index.js"
export { QueueSchema as QueueConfigSchema } from "./generated/index.js"

/**
 * Represents a message queue resource with minimal, portable semantics.
 * Queue integrates with platform-native services (AWS SQS, GCP Pub/Sub, Azure Service Bus).
 */
export class Queue {
  private _config: Partial<QueueConfig> = {}

  /**
   * Creates a new Queue builder.
   * @param id Identifier for the queue. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
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
   * Builds and validates the queue configuration.
   * @returns An immutable Resource representing the configured queue.
   * @throws Error if the queue configuration is invalid.
   */
  public build(): Resource {
    const config = QueueSchema.parse(this._config)

    return new Resource({
      type: "queue",
      ...config,
    })
  }
}
