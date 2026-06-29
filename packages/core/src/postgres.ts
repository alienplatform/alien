import {
  type Postgres as PostgresConfig,
  PostgresSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { Postgres as PostgresConfig, PostgresOutputs } from "./generated/index.js"
export { PostgresSchema as PostgresConfigSchema } from "./generated/index.js"

/**
 * Represents a managed PostgreSQL database. The target platform decides the backend;
 * the database is private and reachable only by same-stack workloads.
 */
export class Postgres {
  private _config: Partial<PostgresConfig> = {
    version: "17",
    storage: "20Gi",
    highAvailability: false,
  }

  /**
   * Creates a new Postgres builder.
   * @param id ID of the database. A database of this name is created on the server.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any postgres resource.
   * Used for creating permission targets that apply to all postgres resources.
   * @returns The "postgres" resource type.
   */
  public static any(): ResourceType {
    return "postgres"
  }

  /**
   * Sets the major engine version ("15" | "16" | "17"). Default `"17"`.
   * The union narrows the generated schema's free-form string for authoring ergonomics;
   * keep it in sync with the supported majors if the Rust `Postgres` resource adds one.
   * @param value The major version.
   * @returns The Postgres builder instance.
   */
  public version(value: "15" | "16" | "17"): this {
    this._config.version = value
    return this
  }

  /**
   * Requests vCPUs (e.g. "0.5", "2"). On GCP and Azure, `cpu` and `memory` together pick the
   * smallest tier that satisfies both. On AWS (Aurora Serverless v2) sizing comes from `memory`, so
   * `cpu` is ignored.
   * @param value The requested vCPUs.
   * @returns The Postgres builder instance.
   */
  public cpu(value: string): this {
    this._config.cpu = value
    return this
  }

  /**
   * Requests memory (e.g. "1Gi", "8Gi"). Drives the size on AWS (the ACU ceiling) and, together
   * with `cpu`, the tier on GCP and Azure.
   * @param value The requested memory.
   * @returns The Postgres builder instance.
   */
  public memory(value: string): this {
    this._config.memory = value
    return this
  }

  /**
   * Sets allocated storage (e.g. "100Gi"). Default `"20Gi"`. Grow-only.
   * @param value The storage size.
   * @returns The Postgres builder instance.
   */
  public storage(value: string): this {
    this._config.storage = value
    return this
  }

  /**
   * Enables multi-AZ / regional / zone-redundant high availability. Default `false`.
   * @param value Whether to enable HA.
   * @returns The Postgres builder instance.
   */
  public highAvailability(value = true): this {
    this._config.highAvailability = value
    return this
  }

  /**
   * Builds and validates the postgres configuration.
   * @returns An immutable Resource representing the configured database.
   * @throws Error if the configuration is invalid.
   */
  public build(): Resource {
    const config = PostgresSchema.parse(this._config)

    return new Resource({
      type: "postgres",
      ...config,
    })
  }
}
