import {
  type Kv as KvConfig,
  KvSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { KvOutputs, Kv as KvConfig } from "./generated/index.js"
export { KvSchema as KvConfigSchema } from "./generated/index.js"

/**
 * Represents a key-value store for data storage.
 * This resource provides a platform-agnostic interface over cloud-native KV services:
 * - AWS: DynamoDB with hash bucketing for load distribution
 * - GCP: Firestore with document-based storage and collection management  
 * - Azure: Table Storage with client-side TTL filtering and partition fan-out
 * 
 * The KV store supports basic operations: get, put, delete, exists, and scan_prefix.
 * All operations support TTL for automatic expiration and conditional operations.
 * 
 * Key Features:
 * - Universal size limits: 512B keys, 64KB values
 * - TTL support with logical expiry across all platforms
 * - Conditional puts with if_not_exists support
 * - Prefix-based scanning with pagination
 * - Platform-specific optimizations while maintaining consistent API
 * 
 * Size Constraints:
 * - Keys: ≤ 512 bytes with portable ASCII charset (a-z, A-Z, 0-9, -, _, :, /, .)
 * - Values: ≤ 65,536 bytes (64 KiB)
 * 
 * TTL Behavior:
 * - Expired items appear absent on reads even if physically present
 * - TTL precision varies by platform but logical behavior is consistent
 * 
 * Scan Operations:
 * - Returns arbitrary, unordered subsets in backend-natural order
 * - No ordering guarantees across platforms
 * - May return ≤ limit items (not guaranteed to fill)
 * - Clients must de-duplicate keys across pages
 * - No completeness guarantee under concurrent writes
 */
export class Kv {
  private _config: Partial<KvConfig> = {}

  /**
   * Creates a new KV builder.
   * @param id Identifier for the KV store. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any KV resource.
   * Used for creating permission targets that apply to all KV resources.
   * @returns The "kv" resource type.
   */
  public static any(): ResourceType {
    return "kv"
  }

  /**
   * Builds and validates the KV configuration.
   * @returns An immutable Resource representing the configured KV store.
   * @throws Error if the KV configuration is invalid.
   */
  public build(): Resource {
    const config = KvSchema.parse(this._config)

    return new Resource({
      type: "kv",
      ...config,
    })
  }
}
