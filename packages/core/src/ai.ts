import {
  type Ai as AiConfig,
  AiSchema,
  type FinetuneMethod,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { AiOutputs, Ai as AiConfig, FinetuneSpec, FinetuneMethod } from "./generated/index.js"
export { AiSchema as AiConfigSchema } from "./generated/index.js"

/**
 * Options for {@link AI.finetune}. `trainingData` accepts either a built
 * {@link Resource} (a Storage bucket) or its id string.
 */
export interface FinetuneOptions {
  /**
   * Provider-native base-model id to tune (an Amazon Nova id on Bedrock, a
   * Gemini model on Vertex, or a gpt-4o family model on Foundry).
   */
  baseModel: string
  /** The Storage resource (or its id) holding the JSONL training dataset. */
  trainingData: Resource | string
  /** Object key of the training file within `trainingData`. Defaults to `training.jsonl`. */
  trainingKey?: string
  /** Public model id apps use to invoke the tuned model. Defaults to `<ai-id>-tuned`. */
  servedModelId?: string
  /** The fine-tuning method. Defaults to `"sft"` (supervised fine-tuning). */
  method?: FinetuneMethod
}

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
   * Declares that this resource should fine-tune a base model in the customer's
   * cloud before serving it. The tuning job reads the JSONL dataset from the
   * given Storage bucket (S3 / GCS / Blob) under the workload's ambient
   * identity — the data never leaves the customer's cloud — and the tuned model
   * is served through the same gateway under `servedModelId` (default
   * `<id>-tuned`). Omit this call for a pure inference gateway.
   *
   * @param options Fine-tuning configuration.
   * @returns This builder, for chaining.
   */
  public finetune(options: FinetuneOptions): this {
    const trainingData =
      typeof options.trainingData === "string"
        ? options.trainingData
        : options.trainingData.config.id

    this._config.finetune = {
      baseModel: options.baseModel,
      trainingData,
      ...(options.trainingKey !== undefined ? { trainingKey: options.trainingKey } : {}),
      ...(options.servedModelId !== undefined ? { servedModelId: options.servedModelId } : {}),
      ...(options.method !== undefined ? { method: options.method } : {}),
    }
    return this
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
