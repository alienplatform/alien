import type { BaseResource } from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

// `alien/` is Alien's own namespace, and `experimental/` holds built-in types that are not
// stable yet, such as `experimental/aws-opensearch`. Alien's Rust core refuses the same names.
const RESERVED_EXTENSION_NAMES = ["alien", "experimental"]

/**
 * The base of the builders `alien extensions build` generates for an extension's resource
 * types. A builder that extends it works like Alien's own: its resource goes into
 * `Stack.add()` and `Worker.link()`, and it offers `enabled()`, `displayName()` and
 * `description()`.
 *
 * A subclass keeps its config, parses it in `build()`, and passes it with its type, named
 * `<extension>/<type>` such as `acme/dns-zone`, to `resource()`.
 */
export abstract class CustomResourceType extends ResourceBuilder {
  /**
   * Builds the Resource for a config whose type an extension defines.
   * @param config The resource's type, id and fields.
   * @returns An immutable Resource.
   * @throws Error if the type is not `<extension>/<type>` under an unreserved extension name.
   */
  protected override resource(config: BaseResource): Resource {
    const slash = config.type.indexOf("/")
    const extension = config.type.slice(0, slash)
    const name = config.type.slice(slash + 1)
    if (
      slash === -1 ||
      extension === "" ||
      name === "" ||
      RESERVED_EXTENSION_NAMES.includes(extension)
    ) {
      throw new Error(
        `Resource type '${config.type}' must be named '<extension>/<type>', such as 'acme/dns-zone'; the extension names 'alien' and 'experimental' are reserved`,
      )
    }
    return super.resource(config)
  }
}
