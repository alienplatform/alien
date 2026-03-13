import {
  type Build as BuildConfig,
  BuildSchema,
  type ComputeType,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { BuildOutputs, ComputeType, BuildStatus, BuildConfig } from "./generated/index.js"
export { BuildOutputsSchema, ComputeTypeSchema, BuildStatusSchema, BuildConfigSchema } from "./generated/index.js"

/**
 * Represents a build resource that executes bash scripts to build code.
 * Builds are designed to be stateless and can be triggered on-demand to compile,
 * test, or package application code.
 */
export class Build {
  private _config: Partial<BuildConfig> = {
    links: [],
    environment: {},
  }

  /**
   * Creates a new Build builder.
   * @param id Identifier for the build resource. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any build resource.
   * Used for creating permission targets that apply to all build resources.
   * @returns The "build" resource type.
   */
  public static any(): ResourceType {
    return "build"
  }

  /**
   * Sets the compute type for the build.
   * @param type The compute type (small, medium, large, x-large).
   * @returns The Build builder instance.
   */
  public computeType(type: ComputeType): this {
    this._config.computeType = type
    return this
  }

  /**
   * Sets key-value pairs as environment variables for the build.
   * @param env A map of environment variable names to their values.
   * @returns The Build builder instance.
   */
  public environment(env: Record<string, string>): this {
    this._config.environment = env
    return this
  }

  /**
   * Links another resource (e.g., Storage, ArtifactRegistry, Role) to this build.
   * This makes the linked resource accessible to the build, often by injecting
   * environment variables or granting permissions.
   * @param resource The resource to link.
   * @returns The Build builder instance.
   */
  public link(resource: Resource): this {
    if (!this._config.links) {
      this._config.links = []
    }
    this._config.links.push(resource.ref())
    return this
  }

  /**
   * Assigns a permission profile to this build.
   * The profile defines the permissions granted to this build when interacting
   * with other cloud resources.
   * @param permissions The permission profile name.
   * @returns The Build builder instance.
   */
  public permissions(permissions: string): this {
    this._config.permissions = permissions
    return this
  }

  /**
   * Builds and validates the build configuration.
   * @returns An immutable Resource representing the configured build.
   * @throws Error if the build configuration is invalid.
   */
  public build(): Resource {
    const config = BuildSchema.parse(this._config)

    return new Resource({
      type: "build",
      ...config,
    })
  }
} 