import {
  type AwsOpenSearchCapacity,
  type AwsOpenSearchCollectionType,
  type AwsOpenSearch as AwsOpenSearchConfig,
  AwsOpenSearchSchema,
  type ResourceType,
} from "../generated/index.js"
import { Resource } from "../resource.js"

export type {
  AwsOpenSearch as AwsOpenSearchConfig,
  AwsOpenSearchCapacity,
  AwsOpenSearchCapacityRange,
  AwsOpenSearchCollectionType,
  AwsOpenSearchOutputs,
} from "../generated/index.js"
export { AwsOpenSearchSchema as AwsOpenSearchConfigSchema } from "../generated/index.js"

/**
 * An Amazon OpenSearch Serverless collection (next generation): compute and
 * storage are decoupled, and the collection lives inside a dedicated
 * collection group. By default, its compute can scale to zero.
 *
 * Experimental and AWS-only: deploying a stack containing this resource to any
 * other platform fails at template generation with a clear error.
 *
 * The collection endpoint is public but every request must be SigV4-signed
 * (signing service name `aoss`, not `es`, and body-carrying requests need an
 * `x-amz-content-sha256` header — official OpenSearch clients with an `aoss`
 * signer handle both) and pass both IAM and the collection's data-access
 * policy. Grant workers access with the
 * `experimental/aws-opensearch/data-access` permission set.
 *
 * The id becomes part of the physical collection name: it must start with a
 * lowercase letter, contain only lowercase letters, digits, and hyphens, and
 * be at most 23 characters.
 */
export class AwsOpenSearch {
  private _config: Partial<AwsOpenSearchConfig> = {
    collectionType: "search",
  }

  /**
   * Creates a new AwsOpenSearch builder.
   * @param id Identifier for the collection (lowercase letters, digits, and hyphens; max 23 characters).
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any AwsOpenSearch resource.
   * Used for creating permission targets that apply to all AwsOpenSearch resources.
   * @returns The "experimental/aws-opensearch" resource type.
   */
  public static any(): ResourceType {
    return "experimental/aws-opensearch"
  }

  /**
   * Sets the workload type of the collection ("search" | "vectorSearch").
   * Immutable once the resource exists. Default `"search"`.
   * @param value The collection type.
   * @returns The AwsOpenSearch builder instance.
   */
  public collectionType(value: AwsOpenSearchCollectionType): this {
    this._config.collectionType = value
    return this
  }

  /**
   * Sets the collection group's indexing and search capacity limits.
   *
   * Omit this setting to retain the provider defaults, including scale-to-zero.
   * Set a minimum of 1 OCU for latency-sensitive workloads that should remain
   * warm.
   *
   * @param value Indexing and search capacity limits.
   * @returns The AwsOpenSearch builder instance.
   */
  public capacity(value: AwsOpenSearchCapacity): this {
    validateCapacity(value)
    this._config.capacity = value
    return this
  }

  /**
   * Builds and validates the collection configuration.
   * @returns An immutable Resource representing the configured collection.
   * @throws Error if the configuration is invalid.
   */
  public build(): Resource {
    const config = AwsOpenSearchSchema.parse(this._config)

    return new Resource({
      type: "experimental/aws-opensearch",
      ...config,
    })
  }
}

function validateCapacity(capacity: AwsOpenSearchCapacity): void {
  const components = [
    ["indexing", capacity.indexing],
    ["search", capacity.search],
  ] as const

  if (components.every(([, range]) => range == null)) {
    throw new Error("OpenSearch capacity must configure indexing, search, or both")
  }

  for (const [component, range] of components) {
    if (range == null) {
      continue
    }
    if (range.minOcu == null && range.maxOcu == null) {
      throw new Error(`OpenSearch ${component} capacity must configure minOcu, maxOcu, or both`)
    }

    if (range.minOcu != null) {
      validateOcu(`${component}.minOcu`, range.minOcu, true)
    }
    if (range.maxOcu != null) {
      validateOcu(`${component}.maxOcu`, range.maxOcu, false)
    }
    if (range.minOcu != null && range.maxOcu != null && range.minOcu > range.maxOcu) {
      throw new Error(`OpenSearch ${component} minOcu must not exceed maxOcu`)
    }
  }
}

function validateOcu(field: string, value: number, allowZero: boolean): void {
  const supported =
    Number.isInteger(value) &&
    value <= 1696 &&
    ((allowZero && value === 0) ||
      value === 1 ||
      value === 2 ||
      value === 4 ||
      value === 8 ||
      value === 16 ||
      (value > 16 && value % 16 === 0))

  if (!supported) {
    throw new Error(
      `OpenSearch ${field} must be ${allowZero ? "0, " : ""}1, 2, 4, 8, 16, or a multiple of 16 up to 1696`,
    )
  }
}
