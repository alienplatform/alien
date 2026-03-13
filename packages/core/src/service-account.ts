import {
  type ServiceAccount as ServiceAccountConfig,
  ServiceAccountSchema,
  type ResourceType,
  type PermissionSet,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { ServiceAccount as ServiceAccountConfig, ServiceAccountOutputs, PermissionSet } from "./generated/index.js"
export { ServiceAccountSchema as ServiceAccountConfigSchema, ServiceAccountOutputsSchema, PermissionSetSchema } from "./generated/index.js"

/**
 * Represents a non-human identity that can be assumed by compute services.
 * 
 * Maps to:
 * - AWS: IAM Role
 * - GCP: Service Account  
 * - Azure: User-assigned Managed Identity
 * 
 * ServiceAccounts can be used to grant specific permissions to compute resources
 * and can be impersonated by other services for cross-account or cross-service access.
 * 
 * @example
 * ```typescript
 * import { ServiceAccount, Function } from "@alien/core"
 * 
 * // Create a service account with stack-level permissions
 * const dataProcessorAccount = new ServiceAccount("data-processor")
 *   .build()
 * 
 * // Link it to a function to use its identity
 * const processor = new Function("processor")
 *   .code({ type: "image", image: "processor:latest" })
 *   .link(dataProcessorAccount)
 *   .build()
 * ```
 */
export class ServiceAccount {
  private _config: Partial<ServiceAccountConfig> = {
    stackPermissionSets: [],
  }

  /**
   * Creates a new ServiceAccount builder.
   * @param id Identifier for the service account. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any service account resource.
   * Used for creating permission targets that apply to all service account resources.
   * @returns The "service-account" resource type.
   */
  public static any(): ResourceType {
    return "service-account"
  }

  /**
   * Adds a stack-level permission set to the service account.
   * Stack-level permissions apply to all resources in the stack.
   * 
   * Note: Only inline permission set objects are supported. To use built-in permission sets,
   * you need to resolve them first through the permission set registry or use permission profiles
   * in your stack configuration.
   * 
   * @example
   * ```typescript
   * import { ServiceAccount, type PermissionSet } from "@alien/core"
   * 
   * const customPermissionSet: PermissionSet = {
   *   id: "custom/my-perms",
   *   description: "Custom permissions",
   *   platforms: {
   *     aws: [{ grant: { actions: ["s3:GetObject"] }, binding: { stack: { resources: ["*"] } } }]
   *   }
   * }
   * 
   * const account = new ServiceAccount("custom-processor")
   *   .stackPermissionSet(customPermissionSet)
   *   .build()
   * ```
   * 
   * @param permissionSet The permission set object with platform-specific permissions
   * @returns The ServiceAccount builder instance.
   */
  public stackPermissionSet(permissionSet: PermissionSet): this {
    if (!this._config.stackPermissionSets) {
      this._config.stackPermissionSets = []
    }
    this._config.stackPermissionSets.push(permissionSet)
    return this
  }

  /**
   * Builds and validates the service account configuration.
   * @returns An immutable Resource representing the configured service account.
   * @throws Error if the service account configuration is invalid.
   */
  public build(): Resource {
    const config = ServiceAccountSchema.parse(this._config)

    return new Resource({
      type: "service-account",
      ...config,
    })
  }
}

