import { CommandsClient } from "@alienplatform/sdk/commands"
import { alien, config } from "./config"

/**
 * Get a commands client for a specific deployment.
 *
 * Discovers the deployment's command endpoint via the Platform SDK,
 * then creates a client configured to communicate with that deployment.
 */
export async function getCommandsClient(deploymentId: string) {
  const info = await alien.deployments.getInfo({
    workspace: config.workspace,
    id: deploymentId,
  })

  return new CommandsClient({
    managerUrl: info.arc?.url || config.alienApiUrl,
    deploymentId: info.arc?.deploymentId || deploymentId,
    token: config.alienToken,
    allowLocalStorage: config.alienApiUrl.includes("localhost"),
  })
}

/**
 * Invoke a command on a specific deployment.
 *
 * This is the core function for control plane → deployment communication.
 */
export async function invokeCommand<T = unknown>(
  deploymentId: string,
  command: string,
  params: Record<string, unknown>,
): Promise<T> {
  const client = await getCommandsClient(deploymentId)
  return (await client.invoke(command, params)) as T
}

// Types for deployment commands
export interface IntegrationConfig {
  owner: string
  repo: string
  token?: string
  baseUrl?: string
}

export interface AnalysisMetrics {
  totalPRs: number
  bySize: {
    small: number
    medium: number
    large: number
  }
  byRisk: {
    low: number
    medium: number
    high: number
    critical: number
  }
  avgTimeToFirstReviewHours: number
  avgMergeTimeHours: number
  reviewThroughputScore: number
  churnHotspots: Array<{ file: string; changes: number }>
}

export interface ClassifiedPR {
  number: number
  title: string
  state: string
  url: string
  createdAt: string
  mergedAt?: string
  additions: number
  deletions: number
  changedFiles: number
  size: "small" | "medium" | "large"
  risk: "low" | "medium" | "high" | "critical"
}
