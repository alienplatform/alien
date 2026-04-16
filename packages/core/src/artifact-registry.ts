import {
  type ArtifactRegistry as ArtifactRegistryConfig,
  ArtifactRegistrySchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  ArtifactRegistryOutputs,
  ArtifactRegistry as ArtifactRegistryConfig,
} from "./generated/index.js"
export { ArtifactRegistrySchema as ArtifactRegistryConfigSchema } from "./generated/index.js"

/**
 * Represents an artifact registry for storing container images and other build artifacts.
 * This is a high-level wrapper resource that provides a cloud-agnostic interface over
 * AWS ECR, GCP Artifact Registry, and Azure Container Registry.
 */
export class ArtifactRegistry {
  private _config: Partial<ArtifactRegistryConfig> = {}

  /**
   * Creates a new ArtifactRegistry builder.
   * @param id Identifier for the artifact registry. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any artifact registry resource.
   * Used for creating permission targets that apply to all artifact registry resources.
   * @returns The "artifact-registry" resource type.
   */
  public static any(): ResourceType {
    return "artifact-registry"
  }

  /**
   * AWS-only: Configure ECR private image replication to additional regions.
   * Ensures images pushed in the registry's home region are automatically
   * available in these destination regions (required when Lambda or other
   * compute runs in a different region).
   * @param regions - AWS region codes to replicate to (e.g., ["us-east-2", "eu-west-1"])
   */
  public replicationRegions(regions: string[]): this {
    this._config.replicationRegions = regions
    return this
  }

  /**
   * Builds and validates the artifact registry configuration.
   * @returns An immutable Resource representing the configured artifact registry.
   * @throws Error if the artifact registry configuration is invalid.
   */
  public build(): Resource {
    const config = ArtifactRegistrySchema.parse(this._config)

    return new Resource({
      type: "artifact-registry",
      ...config,
    })
  }
}
