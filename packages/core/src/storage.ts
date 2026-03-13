import {
  type LifecycleRule,
  type Storage as StorageConfig,
  StorageSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { LifecycleRule, StorageOutputs, Storage as StorageConfig } from "./generated/index.js"
export { StorageSchema as StorageConfigSchema } from "./generated/index.js"

/**
 * Represents an object storage bucket.
 */
export class Storage {
  private _config: Partial<StorageConfig> = {
    publicRead: false,
    versioning: false,
    lifecycleRules: [],
  }

  /**
   * Creates a new Storage builder.
   * @param id ID of the storage bucket. For names with dots, each dot-separated label must be ≤ 63 characters.
   */
  constructor(id: string) {
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
   * Builds and validates the storage configuration.
   * @returns An immutable Resource representing the configured storage bucket.
   * @throws Error if the storage configuration is invalid.
   */
  public build(): Resource {
    const config = StorageSchema.parse(this._config)

    return new Resource({
      type: "storage",
      ...config,
    })
  }
}
