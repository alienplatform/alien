import { type Ai as AiConfig, AiSchema, type ResourceType } from "./generated/index.js"
import { Resource } from "./resource.js"

export type { AiOutputs, Ai as AiConfig } from "./generated/index.js"
export { AiSchema as AiConfigSchema } from "./generated/index.js"

/**
 * Represents an AI Gateway resource that provides a unified interface to
 * managed AI inference services across cloud providers.
 */
export class AI {
  private _config: Partial<AiConfig> = {}

  /**
   * Creates a new AI builder.
   * @param id Identifier for the AI resource. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any AI resource.
   * Used for creating permission targets that apply to all AI resources.
   * @returns The "ai" resource type.
   */
  public static any(): ResourceType {
    return "ai"
  }

  /**
   * Builds and validates the AI configuration.
   * @returns An immutable Resource representing the configured AI Gateway.
   * @throws Error if the AI configuration is invalid.
   */
  public build(): Resource {
    const config = AiSchema.parse(this._config)

    return new Resource({
      type: "ai",
      ...config,
    })
  }
}
