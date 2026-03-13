/**
 * Core types for @alienplatform/testing
 */

import type { Platform } from "@alienplatform/core"

export type { Platform }

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
 * Platform credentials (optional)
 *
 * When not provided, deployers use standard environment variables.
 */
export type PlatformCredentials =
  | {
      platform: "aws"
      accessKeyId: string
      secretAccessKey: string
      region: string
      sessionToken?: string
    }
  | {
      platform: "gcp"
      projectId: string
      region: string
      serviceAccountKeyPath?: string
      serviceAccountKeyJson?: string
    }
  | {
      platform: "azure"
      subscriptionId: string
      tenantId: string
      clientId: string
      clientSecret: string
      region: string
    }
  | {
      platform: "kubernetes"
      kubeconfigPath?: string
    }
  | {
      platform: "local"
    }
  | {
      platform: "test"
    }

export type AWSCredentials = Extract<PlatformCredentials, { platform: "aws" }>
export type GCPCredentials = Extract<PlatformCredentials, { platform: "gcp" }>
export type AzureCredentials = Extract<PlatformCredentials, { platform: "azure" }>
export type KubernetesCredentials = Extract<PlatformCredentials, { platform: "kubernetes" }>
export type LocalCredentials = Extract<PlatformCredentials, { platform: "local" }>
export type TestCredentials = Extract<PlatformCredentials, { platform: "test" }>

/**
 * Options for deploying an application
 */
export interface DeployOptions {
  /** Path to application directory */
  app: string

  /** Optional: specific config file to use (e.g., alien.config.function.ts) */
  config?: string

  /** Target platform */
  platform: Platform

  /** Platform credentials (optional — falls back to env vars) */
  credentials?: PlatformCredentials

  /** Environment variables */
  environmentVariables?: EnvironmentVariable[]

  /** Verbose logging */
  verbose?: boolean
}

/**
 * Deployment info response from alien-server
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
