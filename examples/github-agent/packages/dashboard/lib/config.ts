import { Alien } from "@aliendotdev/platform-api"

/**
 * Configuration and SDK clients for the GitHub Agent dashboard.
 *
 * Environment variables:
 * - ALIEN_API_URL: Platform API URL (required)
 * - ALIEN_TOKEN: Platform API token (required)
 * - ALIEN_WORKSPACE: Workspace ID (required)
 * - ALIEN_PROJECT: Project ID (required)
 */

// Validate and extract required environment variables
function getRequiredEnv(key: string): string {
  const value = process.env[key]
  if (!value) {
    throw new Error(`Missing required environment variable: ${key}`)
  }
  return value
}

/**
 * Environment configuration
 */
export const config: {
  readonly alienApiUrl: string
  readonly alienToken: string
  readonly workspace: string
  readonly project: string
} = {
  alienApiUrl: getRequiredEnv("ALIEN_API_URL"),
  alienToken: getRequiredEnv("ALIEN_TOKEN"),
  workspace: getRequiredEnv("ALIEN_WORKSPACE"),
  project: getRequiredEnv("ALIEN_PROJECT"),
}

/**
 * Platform SDK client (server-side only)
 *
 * Provides typed access to:
 * - agents.list() - List agents in a deployment group
 * - agents.getInfo() - Get agent connection info (ARC URL, resource URLs)
 * - deploymentGroups.* - Manage deployment groups
 *
 * Note: This will only be initialized on the server (Node.js environment).
 * Do not import this in client components.
 */
export const alien = new Alien({
  serverURL: config.alienApiUrl,
  apiKey: config.alienToken,
})
