/**
 * E2E test configuration from environment variables
 */

import type { Platform, PlatformCredentials } from "@aliendotdev/testing"

export interface E2EConfig {
  /** Skip cleanup after tests (for debugging) */
  skipCleanup: boolean
  /** Verbose logging */
  verbose: boolean
}

export function getE2EConfig(): E2EConfig {
  return {
    skipCleanup: process.env.SKIP_CLEANUP === "true",
    verbose: process.env.VERBOSE === "true",
  }
}

export function getCredentials(platform: Platform): PlatformCredentials | undefined {
  switch (platform) {
    case "aws":
      if (
        process.env.AWS_MANAGEMENT_ACCESS_KEY_ID &&
        process.env.AWS_MANAGEMENT_SECRET_ACCESS_KEY
      ) {
        return {
          platform: "aws",
          accessKeyId: process.env.AWS_MANAGEMENT_ACCESS_KEY_ID,
          secretAccessKey: process.env.AWS_MANAGEMENT_SECRET_ACCESS_KEY,
          region: process.env.AWS_MANAGEMENT_REGION || "us-east-1",
        }
      }
      return undefined

    case "gcp":
      if (process.env.GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY) {
        return {
          platform: "gcp",
          projectId: process.env.GOOGLE_MANAGEMENT_PROJECT_ID,
          region: process.env.GOOGLE_MANAGEMENT_REGION || "us-central1",
          serviceAccountKeyJson: process.env.GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY,
        }
      }
      return undefined

    case "azure":
      if (
        process.env.AZURE_MANAGEMENT_SUBSCRIPTION_ID &&
        process.env.AZURE_MANAGEMENT_TENANT_ID &&
        process.env.AZURE_MANAGEMENT_CLIENT_ID &&
        process.env.AZURE_MANAGEMENT_CLIENT_SECRET
      ) {
        return {
          platform: "azure",
          subscriptionId: process.env.AZURE_MANAGEMENT_SUBSCRIPTION_ID,
          tenantId: process.env.AZURE_MANAGEMENT_TENANT_ID,
          clientId: process.env.AZURE_MANAGEMENT_CLIENT_ID,
          clientSecret: process.env.AZURE_MANAGEMENT_CLIENT_SECRET,
          region: process.env.AZURE_REGION || "eastus",
        }
      }
      return undefined

    case "kubernetes":
      return {
        platform: "kubernetes",
        kubeconfigPath: process.env.KUBECONFIG,
      }

    case "local":
      return { platform: "local" }

    default:
      return undefined
  }
}
