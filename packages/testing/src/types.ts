/**
 * Core types for @alienplatform/testing
 */

/**
 * Target platform for deployment.
 *
 * - 'local' (default) — runs locally via `alien dev`, no credentials needed
 * - 'aws' | 'gcp' | 'azure' — deploys to the cloud via the platform API (requires ALIEN_API_KEY)
 */
export type Platform = "local" | "aws" | "gcp" | "azure"

/**
 * Environment variable configuration for deployments
 */
export interface EnvironmentVariable {
  name: string
  value: string
  type?: "plain" | "secret"
  targetResources?: string[]
}

/**
 * Options for deploying an application
 */
export interface DeployOptions {
  /** Path to application directory */
  app: string

  /** Optional: specific config file to use (e.g., alien.function.ts) */
  config?: string

  /** Target platform (default: 'local') */
  platform?: Platform

  /** Environment variables */
  environmentVariables?: EnvironmentVariable[]

  /** Verbose logging */
  verbose?: boolean
}

/**
 * Options for upgrading a deployment
 */
export interface UpgradeOptions {
  environmentVariables?: EnvironmentVariable[]
}

/**
 * Deployment info response from alien dev server
 */
export interface DeploymentInfo {
  commands: {
    url: string
    deploymentId: string
  }
  resources: Record<
    string,
    {
      resourceType: string
      publicUrl?: string
    }
  >
  status: string
  platform: Platform
}

/**
 * Init params for creating a Deployment instance (internal)
 */
export interface DeploymentInit {
  id: string
  name: string
  url: string
  platform: Platform
  commandsUrl: string
  appPath: string

  // Dev mode
  process?: import("node:child_process").ChildProcess

  // Platform API mode
  apiUrl?: string
  apiKey?: string
}
