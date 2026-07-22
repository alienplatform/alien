import type { BaseResource, ResourceRef } from "./generated/index.js"
import type { StackInputRef } from "./input.js"

export { ResourceTypeSchema } from "./generated/index.js"
export type { ResourceType } from "./generated/index.js"

/**
 * Represents a generic cloud resource within the Alien framework.
 * This class encapsulates the common properties of a resource, such as its name and configuration.
 */
export class Resource {
  /**
   * Id of the boolean stack input that decides whether this resource is created at all.
   * Undefined means always create it.
   *
   * Declared rather than defined because class fields materialise on construction: a
   * plain declaration would put `enabledWhen: undefined` on every resource, including
   * ungated ones, which shows up in the stack snapshots.
   */
  declare readonly enabledWhen?: string

  /**
   * Creates a new Resource instance.
   * @param config The configuration object for this specific resource type.
   * @param enabledWhen Id of the boolean stack input gating this resource's creation.
   */
  constructor(
    public config: BaseResource,
    enabledWhen?: string,
  ) {
    if (enabledWhen !== undefined) {
      this.enabledWhen = enabledWhen
    }
  }

  /**
   * Returns a reference to this resource.
   * A resource reference is used to link resources together (e.g., granting a Worker access to a Storage bucket).
   * @returns A ResourceRef object containing the type and name of this resource.
   */
  public ref(): ResourceRef {
    return { type: this.config.type, id: this.config.id }
  }
}

/**
 * Base class for the per-resource builders, giving every resource type the same
 * `.enabled()` gate without each builder reimplementing it.
 */
export abstract class ResourceBuilder {
  private _enabledWhen?: string

  /**
   * Creates this resource only when the given boolean stack input is true.
   * A deployer who answers no never gets the resource, its outputs, or anything derived from it.
   * A frozen resource's answer is fixed when the deployment is created; a live
   * resource follows later edits to the input, and turning it off deletes the
   * resource together with its data.
   * @param input A boolean stack input declared with alien.inputs({...}).
   * @returns The builder instance.
   */
  public enabled(input: StackInputRef<boolean>): this {
    this._enabledWhen = input.id
    return this
  }

  /**
   * Carries the `.enabled()` gate into the Resource.
   * @param config The validated configuration for this resource type.
   * @returns An immutable Resource.
   */
  protected resource(config: BaseResource): Resource {
    return new Resource(config, this._enabledWhen)
  }
}
