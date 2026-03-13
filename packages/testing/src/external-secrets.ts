/**
 * External secrets - platform-native secret management
 */

import { exec } from "node:child_process"
import { mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { promisify } from "node:util"
import { AlienError } from "@aliendotdev/core"
import { PutParameterCommand, SSMClient } from "@aws-sdk/client-ssm"
import { ClientSecretCredential } from "@azure/identity"
import { SecretClient } from "@azure/keyvault-secrets"
import { SecretManagerServiceClient } from "@google-cloud/secret-manager"
import { CoreV1Api, KubeConfig } from "@kubernetes/client-node"
import {
  TestingOperationFailedError,
  TestingUnsupportedPlatformError,
  withTestingContext,
} from "./errors.js"
import type {
  AWSCredentials,
  AzureCredentials,
  GCPCredentials,
  KubernetesCredentials,
  Platform,
  PlatformCredentials,
} from "./types.js"

const execAsync = promisify(exec)

/**
 * Set an external secret using platform-native tools
 *
 * Uses explicit credentials if provided, otherwise falls back to environment variables.
 */
export async function setExternalSecret(
  platform: Platform,
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  credentials?: PlatformCredentials,
  namespace?: string,
  stateDir?: string,
  agentName?: string,
): Promise<void> {
  try {
    switch (platform) {
      case "aws":
        await setAWSSecret(
          resourcePrefix,
          vaultName,
          secretKey,
          secretValue,
          credentials?.platform === "aws" ? credentials : undefined,
        )
        break

      case "gcp":
        await setGCPSecret(
          resourcePrefix,
          vaultName,
          secretKey,
          secretValue,
          credentials?.platform === "gcp" ? credentials : undefined,
        )
        break

      case "azure":
        await setAzureSecret(
          resourcePrefix,
          vaultName,
          secretKey,
          secretValue,
          credentials?.platform === "azure" ? credentials : undefined,
        )
        break

      case "kubernetes":
        await setKubernetesSecret(
          resourcePrefix,
          vaultName,
          secretKey,
          secretValue,
          credentials?.platform === "kubernetes" ? credentials : undefined,
          namespace,
        )
        break

      case "local":
        await setLocalSecret(vaultName, secretKey, secretValue, stateDir, agentName)
        break

      case "test":
        // No-op for test platform
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
 * Uses explicit credentials if provided, otherwise falls back to environment variables.
 */
async function setAWSSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  credentials?: AWSCredentials,
): Promise<void> {
  const client = new SSMClient({
    region: credentials?.region || process.env.AWS_REGION,
    credentials: credentials
      ? {
          accessKeyId: credentials.accessKeyId,
          secretAccessKey: credentials.secretAccessKey,
          sessionToken: credentials.sessionToken,
        }
      : undefined,
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
 * Uses explicit credentials if provided, otherwise falls back to environment variables.
 */
async function setGCPSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  credentials?: GCPCredentials,
): Promise<void> {
  let client: SecretManagerServiceClient
  let projectId: string | undefined

  if (credentials?.serviceAccountKeyJson) {
    // Write service account key to temp file for SDK
    const tempDir = await mkdtemp(join(tmpdir(), "gcp-creds-"))
    const keyPath = join(tempDir, "sa-key.json")
    await writeFile(keyPath, credentials.serviceAccountKeyJson, "utf8")

    client = new SecretManagerServiceClient({
      keyFilename: keyPath,
    })
    projectId = credentials.projectId
  } else if (credentials?.serviceAccountKeyPath) {
    client = new SecretManagerServiceClient({
      keyFilename: credentials.serviceAccountKeyPath,
    })
    projectId = credentials.projectId
  } else {
    // Falls back to GOOGLE_APPLICATION_CREDENTIALS
    client = new SecretManagerServiceClient()
    projectId = process.env.GCP_PROJECT_ID || process.env.GOOGLE_CLOUD_PROJECT
  }

  if (!projectId) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setGCPSecret",
        message: "GCP project ID is required (via credentials or GCP_PROJECT_ID env var)",
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
 * Uses explicit credentials if provided, otherwise falls back to environment variables.
 */
async function setAzureSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  credentials?: AzureCredentials,
): Promise<void> {
  const vaultNameFull = `${resourcePrefix}-${vaultName}`
  const vaultUrl = `https://${vaultNameFull}.vault.azure.net`

  let credential: ClientSecretCredential
  if (credentials) {
    credential = new ClientSecretCredential(
      credentials.tenantId,
      credentials.clientId,
      credentials.clientSecret,
    )
  } else {
    // Fall back to environment variables
    const tenantId = process.env.AZURE_TENANT_ID
    const clientId = process.env.AZURE_CLIENT_ID
    const clientSecret = process.env.AZURE_CLIENT_SECRET

    if (!tenantId || !clientId || !clientSecret) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "setAzureSecret",
          message:
            "Azure credentials are required (via credentials or AZURE_TENANT_ID/AZURE_CLIENT_ID/AZURE_CLIENT_SECRET env vars)",
        }),
      )
    }

    credential = new ClientSecretCredential(tenantId, clientId, clientSecret)
  }

  const client = new SecretClient(vaultUrl, credential)

  // Azure Key Vault requires alphanumeric names with hyphens
  const azureSecretKey = secretKey.replace(/_/g, "-")

  await client.setSecret(azureSecretKey, secretValue)
}

/**
 * Set Kubernetes secret
 *
 * Uses explicit kubeconfig path if provided, otherwise falls back to:
 * - KUBECONFIG environment variable
 * - ~/.kube/config (default)
 *
 * Note: For Helm deployments, resourcePrefix is the Helm release name
 * Secret naming: {resourcePrefix}-{vaultName}
 */
async function setKubernetesSecret(
  resourcePrefix: string,
  vaultName: string,
  secretKey: string,
  secretValue: string,
  credentials?: KubernetesCredentials,
  namespace?: string,
): Promise<void> {
  const kc = new KubeConfig()

  if (credentials?.kubeconfigPath) {
    kc.loadFromFile(credentials.kubeconfigPath)
  } else {
    // loadFromDefault() uses KUBECONFIG env var or ~/.kube/config
    kc.loadFromDefault()
  }

  const k8sApi = kc.makeApiClient(CoreV1Api)
  const ns = namespace || process.env.KUBERNETES_NAMESPACE || "default"
  const secretName = `${resourcePrefix}-${vaultName}`

  try {
    // Try to get existing secret
    const { body: existingSecret } = await k8sApi.readNamespacedSecret(secretName, ns)

    // Update existing secret
    existingSecret.data = existingSecret.data || {}
    existingSecret.data[secretKey] = Buffer.from(secretValue, "utf8").toString("base64")

    await k8sApi.replaceNamespacedSecret(secretName, ns, existingSecret)
  } catch (error: any) {
    if (error.statusCode === 404) {
      // Secret doesn't exist, create it
      await k8sApi.createNamespacedSecret(ns, {
        metadata: {
          name: secretName,
        },
        type: "Opaque",
        data: {
          [secretKey]: Buffer.from(secretValue, "utf8").toString("base64"),
        },
      })
    } else {
      throw await withTestingContext(
        error,
        "setKubernetesSecret",
        "Failed to set Kubernetes secret",
      )
    }
  }
}

/**
 * Set local dev secret using alien dev vault set
 *
 * @param stateDir - State directory path from running alien dev process (absolute path)
 */
async function setLocalSecret(
  vaultName: string,
  secretKey: string,
  secretValue: string,
  stateDir?: string,
  agentName?: string,
): Promise<void> {
  // Use alien CLI to set the secret
  const alienCliPath = process.env.ALIEN_CLI_PATH || "alien"

  if (!stateDir) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setLocalSecret",
        message: "stateDir is required for local vault set",
      }),
    )
  }

  if (!agentName) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "setLocalSecret",
        message: "agentName is required for local vault set",
      }),
    )
  }

  // The state dir we receive is appPath/.alien
  // The vault command needs the agent-specific state dir: appPath/.alien/agents/<name>/state
  const agentStateDir = `${stateDir}/agents/${agentName}/state`

  // Format: alien dev vault --state-dir <dir> set <vault> <key> <value>
  const command = `${alienCliPath} dev vault --state-dir "${agentStateDir}" set "${vaultName}" "${secretKey}" "${secretValue}"`

  await execAsync(command)
}
