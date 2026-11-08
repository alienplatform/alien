import {
  type Redis as RedisConfig,
  type RedisEvictionPolicy,
  RedisSchema,
  type ResourceType,
} from "./generated/index.js"
import { encryptionKeyRef } from "./key.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type {
  Redis as RedisConfig,
  RedisEvictionPolicy,
  RedisOutputs,
} from "./generated/index.js"
export { RedisSchema as RedisConfigSchema } from "./generated/index.js"

/**
 * Represents a managed Redis-compatible server. The target platform decides the backend; the
 * server is private, reachable only by same-stack workloads, and always uses TLS and a password
 * on the clouds. It is one endpoint (not cluster mode), so standard clients and multi-key
 * scripts work unchanged.
 */
export class Redis extends ResourceBuilder {
  private _config: Partial<RedisConfig> = {
    evictionPolicy: "volatile-lru",
    persistence: false,
    highAvailability: false,
  }

  /**
   * Creates a new Redis builder.
   * @param id ID of the Redis server.
   */
  constructor(id: string) {
    super()
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any redis resource.
   * Used for creating permission targets that apply to all redis resources.
   * @returns The "redis" resource type.
   */
  public static any(): ResourceType {
    return "redis"
  }

  /**
   * Sets the memory available for data (e.g. "512Mi", "4Gi"). The smallest tier that fits is
   * used. A request larger than the biggest single-server tier on the platform is rejected.
   * Use binary units (Ki, Mi, Gi, Ti); a number without a unit means GiB.
   * @param value The requested memory.
   * @returns The Redis builder instance.
   */
  public memory(value: string): this {
    this._config.memory = value
    return this
  }

  /**
   * Sets what the server does when memory is full. Default `"volatile-lru"`: only keys with a
   * TTL are evicted. Use `"noeviction"` for queues (BullMQ) and `"allkeys-lru"` for a pure cache.
   * @param value The eviction policy.
   * @returns The Redis builder instance.
   */
  public evictionPolicy(value: RedisEvictionPolicy): this {
    this._config.evictionPolicy = value
    return this
  }

  /**
   * Keeps data across a server restart or maintenance. Default `false` (a cache). On AWS, GCP
   * and Azure it requires `highAvailability()`. What survives what differs per platform; see the
   * Redis docs.
   * @param value Whether to persist data.
   * @returns The Redis builder instance.
   */
  public persistence(value = true): this {
    this._config.persistence = value
    return this
  }

  /**
   * Adds one replica in another zone with automatic failover. Default `false`.
   * @param value Whether to enable HA.
   * @returns The Redis builder instance.
   */
  public highAvailability(value = true): this {
    this._config.highAvailability = value
    return this
  }

  /** Uses a Key in the same stack to encrypt data at rest. Immutable after creation. */
  public encryptionKey(key: Resource): this {
    this._config.encryptionKey = encryptionKeyRef(key)
    return this
  }

  /**
   * Builds and validates the Redis configuration.
   * @returns An immutable Resource representing the configured server.
   * @throws Error if the configuration is invalid.
   */
  public build(): Resource {
    const config = RedisSchema.parse(this._config)

    return this.resource({
      type: "redis",
      ...config,
    })
  }
}
