import type { BaseResource, ResourceRef } from "./generated/index.js"

export { ResourceTypeSchema } from "./generated/index.js"
export type { ResourceType } from "./generated/index.js"

/**
 * Represents a generic cloud resource within the Alien framework.
 * This class encapsulates the common properties of a resource, such as its name and configuration.
 */
export class Resource {
  /**
   * Creates a new Resource instance.
   * @param config The configuration object for this specific resource type.
   */
  constructor(public config: BaseResource) {}

  /**
   * Returns a reference to this resource.
   * A resource reference is used to link resources together (e.g., granting a Function access to a Storage bucket).
   * @returns A ResourceRef object containing the type and name of this resource.
   */
  public ref(): ResourceRef {
    return { type: this.config.type, id: this.config.id }
  }
}
