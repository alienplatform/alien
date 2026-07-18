import { type Queue as QueueConfig, QueueSchema, type ResourceType } from "./generated/index.js"
import type { StackInputRef } from "./input.js"
import { type ResourceGate, applyResourceGate } from "./permission.js"
import { Resource } from "./resource.js"

export type { QueueOutputs, Queue as QueueConfig } from "./generated/index.js"
export { QueueSchema as QueueConfigSchema } from "./generated/index.js"

/**
 * Represents a message queue resource with minimal, portable semantics.
 * Queue integrates with platform-native services (AWS SQS, GCP Pub/Sub, Azure Service Bus).
 */
export class Queue {
  private _config: Partial<QueueConfig> = {}
  private _enabledWhen?: ResourceGate

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
   * Provision this queue's permission grants only while the boolean input
   * resolves to true. Lowered at stack build into gates on the sets a profile
   * grants for it, so the baked role lacks them when the deployer turns it off.
   * @param input The gating boolean stack input (deployer-provided, with an env mapping).
   */
  public enabled(input: StackInputRef<boolean>): this {
    this._enabledWhen = { inputId: input.id }
    return this
  }

  /**
   * Builds and validates the queue configuration.
   * @returns An immutable Resource representing the configured queue.
   * @throws Error if the queue configuration is invalid.
   */
  public build(): Resource {
    const config = QueueSchema.parse(this._config)
    const base = { type: "queue" as const, ...config }
    applyResourceGate(base, this._enabledWhen)
    return new Resource(base)
  }
}
