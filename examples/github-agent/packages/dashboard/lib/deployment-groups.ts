import { eq } from "drizzle-orm"
import { alien, config } from "./config"
import { db } from "./db"
import { organizationMetadata } from "./schema"

/**
 * Create a deployment group for an organization.
 */
export async function createDeploymentGroupForOrganization(
  organizationId: string,
  organizationName: string,
  organizationSlug: string | null,
): Promise<{
  deploymentGroupId: string
  deploymentToken: string
}> {
  // Create deployment group with organization's name
  const name = organizationSlug ?? organizationName.toLowerCase().replace(/[^a-z0-9-]/g, "-")

  const deploymentGroup = await alien.deploymentGroups.createDeploymentGroup({
    workspace: config.workspace,
    createDeploymentGroupRequest: {
      name,
      project: config.project,
      maxAgents: 10,
    },
  })

  // Create deployment group token
  const tokenResponse = await alien.deploymentGroups.createDeploymentGroupToken({
    workspace: config.workspace,
    id: deploymentGroup.id,
    createDeploymentGroupTokenRequest: {
      description: `Deployment token for ${organizationName}`,
    },
  })

  if (!deploymentGroup.id || !tokenResponse.token) {
    throw new Error("Failed to create deployment group: missing id or token")
  }

  return {
    deploymentGroupId: deploymentGroup.id,
    deploymentToken: tokenResponse.token,
  }
}

/**
 * Get or create deployment group for an organization.
 *
 * Idempotent operation that creates a deployment group on-demand.
 */
export async function getOrCreateDeploymentGroup(
  organizationId: string,
  organizationName: string,
  organizationSlug: string | null,
): Promise<{
  deploymentGroupId: string
  deploymentToken: string
}> {
  // Check if deployment group already exists
  const [metadata] = await db
    .select()
    .from(organizationMetadata)
    .where(eq(organizationMetadata.organizationId, organizationId))
    .limit(1)

  if (metadata?.deploymentGroupId && metadata.deploymentToken) {
    return {
      deploymentGroupId: metadata.deploymentGroupId,
      deploymentToken: metadata.deploymentToken,
    }
  }

  // Create new deployment group
  const result = await createDeploymentGroupForOrganization(
    organizationId,
    organizationName,
    organizationSlug,
  )

  // Store in database (upsert)
  if (metadata) {
    await db
      .update(organizationMetadata)
      .set({
        deploymentGroupId: result.deploymentGroupId,
        deploymentToken: result.deploymentToken,
        updatedAt: new Date(),
      })
      .where(eq(organizationMetadata.organizationId, organizationId))
  } else {
    await db.insert(organizationMetadata).values({
      id: `org_meta_${organizationId}`,
      organizationId,
      deploymentGroupId: result.deploymentGroupId,
      deploymentToken: result.deploymentToken,
      createdAt: new Date(),
      updatedAt: new Date(),
    })
  }

  return result
}
