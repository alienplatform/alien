import { type Key as KeyConfig, type ResourceType, KeySchema } from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type { KeyFingerprint, KeyOutputs, Key as KeyConfig } from "./generated/index.js"
export { KeySchema as KeyConfigSchema } from "./generated/index.js"

/** A customer-managed encryption key. */
export class Key extends ResourceBuilder {
  private _config: Partial<KeyConfig> = {}

  public constructor(id: string) {
    super()
    this._config.id = id
  }

  public static any(): ResourceType {
    return "key"
  }

  public build(): Resource {
    const config = KeySchema.parse(this._config)
    return this.resource({ type: "key", ...config })
  }
}
