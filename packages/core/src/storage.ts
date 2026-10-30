import { z } from "zod/v4"
import { AlienError, defineError } from "./error.js"
import {
  type LifecycleRule,
  type ResourceType,
  type Storage as StorageConfig,
  type StorageNotification,
  StorageSchema,
} from "./generated/index.js"
import { encryptionKeyRef } from "./key.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type {
  LifecycleRule,
  Storage as StorageConfig,
  StorageNotification,
  StorageNotificationEvent,
  StorageOutputs,
} from "./generated/index.js"
export { StorageSchema as StorageConfigSchema } from "./generated/index.js"

/** Options for delivering object creation events into a queue. */
export type StorageNotificationOptions = Partial<Omit<StorageNotification, "queue">>

const InvalidStorageNotification = defineError({
  code: "INVALID_STORAGE_NOTIFICATION",
  context: z.object({ reason: z.string() }),
  message: ({ reason }) => `Invalid storage notification: ${reason}`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/**
 * Represents an object storage bucket.
 */
export class Storage extends ResourceBuilder {
  private _config: Partial<StorageConfig> = {
    publicRead: false,
    versioning: false,
    lifecycleRules: [],
    corsAllowedOrigins: [],
  }

  /**
   * Creates a new Storage builder.
   * @param id ID of the storage bucket. For names with dots, each dot-separated label must be ≤ 63 characters.
   */
  constructor(id: string) {
    super()
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any storage resource.
   * Used for creating permission targets that apply to all storage resources.
   * @returns The "storage" resource type.
   */
  public static any(): ResourceType {
    return "storage"
  }

  /**
   * Allows public read access to objects without authentication.
   * Default: `false`.
   * @param value Whether to allow public read access.
   * @returns The Storage builder instance.
   */
  public publicRead(value: boolean): this {
    this._config.publicRead = value
    return this
  }

  /**
   * Enables object versioning.
   * Default: `false`.
   * @param value Whether to enable versioning.
   * @returns The Storage builder instance.
   */
  public versioning(value: boolean): this {
    this._config.versioning = value
    return this
  }

  /**
   * Defines lifecycle rules for automatic object management (e.g., expiration).
   * @param rules An array of lifecycle rules.
   * @returns The Storage builder instance.
   */
  public lifecycleRules(rules: LifecycleRule[]): this {
    this._config.lifecycleRules = rules
    return this
  }

  /**
   * Allows browser reads from the listed origins through signed URLs.
   * Providers enable `GET` and `HEAD`; use `*` only when the signed URL itself
   * is the credential and browser cookies are not involved.
   * @param origins Allowed browser origins.
   * @returns The Storage builder instance.
   */
  public corsAllowedOrigins(origins: string[]): this {
    this._config.corsAllowedOrigins = origins
    return this
  }

  /**
   * Delivers matching object creation events into a Queue in the same stack.
   * Prefix and suffix are literal and case sensitive. Overlapping rules each deliver
   * a message. Storage and Queue must both be Frozen or both be Live.
   */
  public notify(queue: Resource, options: StorageNotificationOptions = {}): this {
    if (queue.config.type !== "queue") {
      throw new AlienError(
        InvalidStorageNotification.create({
          reason: `expected a Queue, received '${queue.config.type}'`,
        }),
      )
    }
    const notification: StorageNotification = {
      queue: queue.ref(),
      events: options.events === undefined ? ["created"] : [...options.events],
      ...(options.prefix !== undefined ? { prefix: options.prefix } : {}),
      ...(options.suffix !== undefined ? { suffix: options.suffix } : {}),
    }
    this._config.notifications = [...(this._config.notifications ?? []), notification]
    return this
  }

  /**
   * Uses an Alien Key for provider-native encryption of objects in this Storage resource.
   * The Key and Storage must both be added to the same stack.
   */
  public encryptionKey(key: Resource): this {
    this._config.encryptionKey = encryptionKeyRef(key)
    return this
  }

  /**
   * Builds and validates the storage configuration.
   * @returns An immutable Resource representing the configured storage bucket.
   * @throws Error if the storage configuration is invalid.
   */
  public build(): Resource {
    const config = StorageSchema.parse(this._config)

    return this.resource({
      type: "storage",
      ...config,
    })
  }
}
