/**
 * External secrets - platform-native secret management
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { AlienError } from "@alienplatform/core"
import { PutParameterCommand, SSMClient } from "@aws-sdk/client-ssm"
import { ClientSecretCredential } from "@azure/identity"
import { SecretClient } from "@azure/keyvault-secrets"
import { SecretManagerServiceClient } from "@google-cloud/secret-manager"
import {
  TestingOperationFailedError,
  TestingUnsupportedPlatformError,
  withTestingContext,
} from "./errors.js"
import type { Platform } from "./types.js"

/**
 * Set an external secret using platform-native tools
 *
 * Falls back to environment variables for cloud provider credentials.
 */
export async function setExternalSecret(
  platform: Platform,
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  _namespace?: string,
  stateDir?: string,
  deploymentId?: string,
): Promise<void> {
  try {
    switch (platform) {
      case "aws":
        await setAWSSecret(resourcePrefix, vaultName, secretKey, secretValue)
        break

      case "gcp":
        await setGCPSecret(resourcePrefix, vaultName, secretKey, secretValue)
        break

      case "azure":
        await setAzureSecret(resourcePrefix, vaultName, secretKey, secretValue)
        break

      case "local":
        await setLocalSecret(vaultName, secretKey, secretValue, stateDir, deploymentId)
        break

      default: {
        const exhaustive: never = platform
        throw new AlienError(
          TestingUnsupportedPlatformError.create({
            platform: String(exhaustive),
            operation: "setExternalSecret",
          }),
        )
      }
    }
  } catch (error) {
    throw await withTestingContext(error, "setExternalSecret", "Failed to set external secret", {
      platform,
      resourcePrefix,
      vaultName,
      secretKey,
    })
  }
}

/**
 * Set AWS SSM Parameter Store secret
 *
 * Uses environment variables for credentials (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION).
 */
async function setAWSSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
): Promise<void> {
  const client = new SSMClient({
    region: process.env.AWS_REGION,
  })
  const parameterName = `/${resourcePrefix}-${vaultName}-${secretKey}`

  await client.send(
    new PutParameterCommand({
      Name: parameterName,
      Value: secretValue,
      Type: "SecureString",
      Overwrite: true,
    }),
  )
}

/**
 * Set GCP Secret Manager secret
 *
 * Uses environment variables for credentials (GOOGLE_APPLICATION_CREDENTIALS, GCP_PROJECT_ID).
 */
async function setGCPSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
): Promise<void> {
  // Falls back to GOOGLE_APPLICATION_CREDENTIALS
  const client = new SecretManagerServiceClient()
  const projectId = process.env.GCP_PROJECT_ID || process.env.GOOGLE_CLOUD_PROJECT

  if (!projectId) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setGCPSecret",
        message: "GCP project ID is required (set GCP_PROJECT_ID or GOOGLE_CLOUD_PROJECT env var)",
      }),
    )
  }

  const secretName = `${resourcePrefix}-${vaultName}-${secretKey}`
  const parent = `projects/${projectId}`
  const secretPath = `${parent}/secrets/${secretName}`

  try {
    // Try to create the secret first
    await client.createSecret({
      parent,
      secretId: secretName,
      secret: {
        replication: {
          automatic: {},
        },
      },
    })
  } catch (error: any) {
    // Secret already exists, that's fine
    if (!error.message?.includes("ALREADY_EXISTS")) {
      throw await withTestingContext(error, "setGCPSecret", "Failed to create GCP secret")
    }
  }

  // Add secret version
  await client.addSecretVersion({
    parent: secretPath,
    payload: {
      data: Buffer.from(secretValue, "utf8"),
    },
  })
}

/**
 * Set Azure Key Vault secret
 *
 * Uses environment variables for credentials (AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET).
 */
async function setAzureSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
): Promise<void> {
  const vaultNameFull = `${resourcePrefix}-${vaultName}`
  const vaultUrl = `https://${vaultNameFull}.vault.azure.net`

  // Fall back to environment variables
  const tenantId = process.env.AZURE_TENANT_ID
  const clientId = process.env.AZURE_CLIENT_ID
  const clientSecret = process.env.AZURE_CLIENT_SECRET

  if (!tenantId || !clientId || !clientSecret) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setAzureSecret",
        message:
          "Azure credentials are required (set AZURE_TENANT_ID, AZURE_CLIENT_ID, and AZURE_CLIENT_SECRET env vars)",
      }),
    )
  }

  const credential = new ClientSecretCredential(tenantId, clientId, clientSecret)
  const client = new SecretClient(vaultUrl, credential)

  // Azure Key Vault requires alphanumeric names with hyphens
  const azureSecretKey = secretKey.replace(/_/g, "-")

  await client.setSecret(azureSecretKey, secretValue)
}

/**
 * Set local dev secret by writing directly to the vault's secrets.json file.
 *
 * The local vault binding (LocalVault) reads from:
 *   {stateDir}/{deploymentId}/vault/{vaultName}/secrets.json
 *
 * We write to the same path so the running function can read it immediately.
 */
async function setLocalSecret(
  vaultName: string,
  secretKey: string,
  secretValue: string,
  stateDir?: string,
  deploymentId?: string,
): Promise<void> {
  if (!stateDir) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setLocalSecret",
        message: "stateDir is required for local vault set",
      }),
    )
  }

  if (!deploymentId) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setLocalSecret",
        message: "deploymentId is required for local vault set",
      }),
    )
  }

  // Path matches what LocalVault reads: {stateDir}/{deploymentId}/vault/{vaultName}/secrets.json
  const vaultDir = join(stateDir, deploymentId, "vault", vaultName)
  const secretsFile = join(vaultDir, "secrets.json")

  // Read existing secrets or start fresh
  let secrets: Record<string, string> = {}
  if (existsSync(secretsFile)) {
    secrets = JSON.parse(readFileSync(secretsFile, "utf-8"))
  }

  // Set the secret
  secrets[secretKey] = secretValue

  // Write back
  mkdirSync(vaultDir, { recursive: true })
  writeFileSync(secretsFile, JSON.stringify(secrets, null, 2))
}
