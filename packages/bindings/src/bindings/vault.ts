/**
 * Vault binding implementation.
 *
 * Provides secure secret management operations.
 */

import { type Channel, createClient } from "nice-grpc"
import {
  type VaultServiceClient as GeneratedVaultServiceClient,
  VaultServiceDefinition,
} from "../generated/vault.js"
import { wrapGrpcCall } from "../grpc-utils.js"

/**
 * Vault binding for secret management operations.
 *
 * @example
 * ```typescript
 * import { vault } from "@alienplatform/bindings"
 *
 * const secrets = vault("app-secrets")
 *
 * // Get a secret
 * const apiKey = await secrets.get("API_KEY")
 *
 * // Set a secret
 * await secrets.set("DATABASE_URL", "postgres://...")
 *
 * // Delete a secret
 * await secrets.delete("OLD_KEY")
 * ```
 */
export class Vault {
  private readonly client: GeneratedVaultServiceClient
  private readonly bindingName: string

  constructor(channel: Channel, bindingName: string) {
    this.client = createClient(VaultServiceDefinition, channel)
    this.bindingName = bindingName
  }

  /**
   * Get a secret value.
   *
   * @param secretName - Name of the secret
   * @returns Secret value
   */
  async get(secretName: string): Promise<string> {
    return await wrapGrpcCall(
      "VaultService",
      "GetSecret",
      async () => {
        const response = await this.client.getSecret({
          bindingName: this.bindingName,
          secretName,
        })
        return response.value
      },
      { bindingName: this.bindingName, secretName },
    )
  }

  /**
   * Get a secret as JSON.
   *
   * @param secretName - Name of the secret
   * @returns Parsed JSON value
   */
  async getJson<T = unknown>(secretName: string): Promise<T> {
    const value = await this.get(secretName)
    return JSON.parse(value) as T
  }

  /**
   * Set a secret value.
   *
   * @param secretName - Name of the secret
   * @param value - Secret value (string or object for JSON)
   */
  async set(secretName: string, value: string | object): Promise<void> {
    const stringValue = typeof value === "string" ? value : JSON.stringify(value)

    await wrapGrpcCall(
      "VaultService",
      "SetSecret",
      async () => {
        await this.client.setSecret({
          bindingName: this.bindingName,
          secretName,
          value: stringValue,
        })
      },
      { bindingName: this.bindingName, secretName },
    )
  }

  /**
   * Delete a secret.
   *
   * @param secretName - Name of the secret to delete
   */
  async delete(secretName: string): Promise<void> {
    await wrapGrpcCall(
      "VaultService",
      "DeleteSecret",
      async () => {
        await this.client.deleteSecret({
          bindingName: this.bindingName,
          secretName,
        })
      },
      { bindingName: this.bindingName, secretName },
    )
  }

  /**
   * Check if a secret exists.
   *
   * @param secretName - Name of the secret
   * @returns True if the secret exists
   */
  async exists(secretName: string): Promise<boolean> {
    try {
      await this.get(secretName)
      return true
    } catch (error) {
      if (error instanceof Error && "code" in error && (error as any).code === "SECRET_NOT_FOUND") {
        return false
      }
      throw error
    }
  }
}
