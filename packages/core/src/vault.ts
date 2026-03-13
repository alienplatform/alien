import {
  type Vault as VaultConfig,
  VaultSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { VaultOutputs, Vault as VaultConfig } from "./generated/index.js"
export { VaultSchema as VaultConfigSchema } from "./generated/index.js"

/**
 * Represents a secure vault for storing secrets.
 * This resource provides a platform-agnostic interface over cloud-native secret management services:
 * - AWS: AWS Secrets Manager with prefixed secret names
 * - GCP: Secret Manager with prefixed secret names
 * - Azure: Key Vault resource
 *
 * The vault acts as a namespace for secrets and controls access permissions for functions and services.
 */
export class Vault {
  private _config: Partial<VaultConfig> = {}

  /**
   * Creates a new Vault builder.
   * @param id Identifier for the vault. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any vault resource.
   * Used for creating permission targets that apply to all vault resources.
   * @returns The "vault" resource type.
   */
  public static any(): ResourceType {
    return "vault"
  }

  /**
   * Builds and validates the vault configuration.
   * @returns An immutable Resource representing the configured vault.
   * @throws Error if the vault configuration is invalid.
   */
  public build(): Resource {
    const config = VaultSchema.parse(this._config)

    return new Resource({
      type: "vault",
      ...config,
    })
  }
}
