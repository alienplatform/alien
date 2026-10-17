import { z } from "zod/v4"
import { AlienError, defineError } from "./error.js"
import {
  type Key as KeyConfig,
  KeySchema,
  type ResourceRef,
  type ResourceType,
} from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type { Key as KeyConfig, KeyFingerprint, KeyOutputs } from "./generated/index.js"
export { KeySchema as KeyConfigSchema } from "./generated/index.js"

export interface EncryptionKeyOptions {
  /**
   * Keep installed resources without a customer-managed key unchanged when this
   * release adds encryption. New resources use the declared key. Existing keys
   * still cannot be removed or replaced.
   */
  allowUnencryptedExisting?: boolean
}

const InvalidEncryptionKey = defineError({
  code: "INVALID_ENCRYPTION_KEY",
  context: z.object({ resourceType: z.string() }),
  message: ({ resourceType }) => `Encryption requires an alien.Key, received '${resourceType}'`,
  retryable: false,
  internal: false,
  httpStatusCode: 400,
})

/** Validates and returns the reference shared by resources with native encryption. */
export function encryptionKeyRef(key: Resource): ResourceRef {
  if (key.config.type !== "key") {
    throw new AlienError(InvalidEncryptionKey.create({ resourceType: key.config.type }))
  }
  return key.ref()
}

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
