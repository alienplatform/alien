import { alien, config } from "./config"
import { db } from "./db"
import { organizationMetadata } from "./schema"
import { eq } from "drizzle-orm"

/**
 * Agent type from the Platform SDK
 */
export type Agent = {
  id: string
  name: string
  platform: string
  status: string
  deploymentGroupId: string
  createdAt: string
}

/**
 * List all agents in an organization's deployment group.
 * Returns empty array if organization has no deployment group.
 */
export async function listAgents(organizationId: string): Promise<Agent[]> {
  // Get deployment group for organization
  const [metadata] = await db
    .select()
    .from(organizationMetadata)
    .where(eq(organizationMetadata.organizationId, organizationId))
    .limit(1)

  if (!metadata?.deploymentGroupId) {
    return []
  }

  const response = await alien.deployments.list({
    workspace: config.workspace,
    deploymentGroup: metadata.deploymentGroupId,
  })

  return response.items.map((agent) => ({
    id: agent.id,
    name: agent.name,
    platform: agent.platform,
    status: agent.status,
    deploymentGroupId: agent.deploymentGroupId,
    createdAt: agent.createdAt.toISOString(),
  }))
}

