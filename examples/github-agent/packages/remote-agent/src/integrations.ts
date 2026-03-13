import { vault } from "@aliendotdev/bindings"
import type { IntegrationConfig } from "./types.js"

const VAULT_NAME = "integrations"

export async function saveIntegrationConfig(
  integrationId: string,
  config: IntegrationConfig,
): Promise<void> {
  const integrations = await vault(VAULT_NAME)
  await integrations.set(integrationId, config)
}

export async function loadIntegrationConfig(integrationId: string): Promise<IntegrationConfig> {
  const integrations = await vault(VAULT_NAME)

  try {
    return await integrations.getJson<IntegrationConfig>(integrationId)
  } catch (error) {
    const message =
      error instanceof Error
        ? error.message
        : "Integration configuration not found"
    throw new Error(`Missing integration ${integrationId}: ${message}`)
  }
}
