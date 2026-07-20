import {
  type AwsOpenSearchCollectionType,
  type AwsOpenSearch as AwsOpenSearchConfig,
  AwsOpenSearchSchema,
  type ResourceType,
} from "../generated/index.js"
import { Resource } from "../resource.js"

export type {
  AwsOpenSearch as AwsOpenSearchConfig,
  AwsOpenSearchCollectionType,
  AwsOpenSearchOutputs,
} from "../generated/index.js"
export { AwsOpenSearchSchema as AwsOpenSearchConfigSchema } from "../generated/index.js"

/**
 * An Amazon OpenSearch Serverless collection (next generation): compute and
 * storage are decoupled, the collection scales to zero, and it lives inside a
 * dedicated collection group.
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
