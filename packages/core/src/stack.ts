import {
  type PermissionsConfig,
  type Platform,
  type ResourceEntry,
  type ResourceLifecycle,
  type Stack as StackConfig,
  StackSchema,
} from "./generated/index.js"
import type { Resource } from "./resource.js"

/**
 * Options for adding a resource to a stack.
 */
export interface AddResourceOptions {
  /**
   * Enable remote bindings for this resource (BYOB use case).
   * When true, binding params are synced to StackState for external access.
   * Default: false (prevents sensitive data in synced state).
   */
  remoteAccess?: boolean
}

export type {
  Stack as StackConfig,
  StackState,
  StackStatus,
  StackResourceState,
  ResourceStatus,
  PermissionSet,
  ManagementPermissions,
  PermissionsConfig,
} from "./generated/index.js"
export {
  StackSchema,
  StackStateSchema,
  StackStatusSchema,
  StackResourceStateSchema,
  ResourceStatusSchema,
} from "./generated/index.js"

/**
 * Represents a collection of cloud resources that are managed together.
 * Stacks are the top-level organizational unit in an Alien application.
 */
export class Stack {
  private _config: Partial<StackConfig> = {
    resources: {},
    permissions: undefined,
  }

  /**
   * Creates a new Stack builder.
   * @param id Identifier for the stack. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 128 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Adds a resource to the stack with a specified lifecycle.
   * @param resource The resource to add (e.g., Function, Storage).
   * @param lifecycle The lifecycle state of the resource (e.g., Frozen, Live).
   * @param options Optional configuration for the resource entry.
   * @returns The Stack builder instance.
   */
  public add(resource: Resource, lifecycle: ResourceLifecycle, options?: AddResourceOptions): this {
    const entry: ResourceEntry = {
      config: resource.config,
      lifecycle,
      dependencies: [], // Additional dependencies beyond what the resource itself defines
    }
    if (options?.remoteAccess) {
      entry.remoteAccess = true
    }
    this._config.resources![resource.config.id] = entry
    return this
  }

  /**
   * Declare which platforms this stack supports.
   * When omitted, the stack supports all platforms.
   */
  public platforms(platforms: Platform[]): this {
    this._config.supportedPlatforms = platforms
    return this
  }

  /**
   * Configure permissions for this stack.
   * @param config Permission configuration
   * @returns The Stack builder instance.
   */
  public permissions(config: PermissionsConfig): this {
    this._config.permissions = config
    return this
  }

  /**
   * Gets the stack ID without building/validating the stack.
   * @returns The stack ID.
   */
  public get id(): string {
    return this._config.id!
  }

  /**
   * Builds and validates the stack configuration.
   * @returns The complete and validated stack configuration.
   * @throws Error if the stack configuration is invalid.
   */
  public build(): StackConfig {
    return StackSchema.parse(this._config)
  }
}
