import { eq } from "drizzle-orm"
import { alien, config } from "./config"
import { db } from "./db"
import { organizationMetadata } from "./schema"

// The API's default deployment token lifetime, requested explicitly so the
// expiry can be stored next to the token.
const TOKEN_LIFETIME_MS = 7 * 24 * 60 * 60 * 1000
// The token goes into a command the customer may run later, so renew it a day early.
const RENEW_BEFORE_MS = 24 * 60 * 60 * 1000

async function createDeploymentToken(
  deploymentGroupId: string,
  organizationName: string,
): Promise<{ deploymentToken: string; deploymentTokenExpiresAt: Date }> {
  const expiresAt = new Date(Date.now() + TOKEN_LIFETIME_MS)
  const tokenResponse = await alien.deploymentGroups.createDeploymentGroupToken({
    workspace: config.workspace,
    id: deploymentGroupId,
    createDeploymentGroupTokenRequest: {
      description: `Deployment token for ${organizationName}`,
      expiresAt,
    },
  })

  if (!tokenResponse.token) {
    throw new Error("Failed to create deployment token: missing token")
  }

  return { deploymentToken: tokenResponse.token, deploymentTokenExpiresAt: expiresAt }
}

export async function createDeploymentGroupForOrganization(
  organizationId: string,
  organizationName: string,
  organizationSlug: string | null,
): Promise<{
  deploymentGroupId: string
  deploymentToken: string
  deploymentTokenExpiresAt: Date
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

  if (!deploymentGroup.id) {
    throw new Error("Failed to create deployment group: missing id")
  }

  return {
    deploymentGroupId: deploymentGroup.id,
    ...(await createDeploymentToken(deploymentGroup.id, organizationName)),
  }
}

/**
 * Get or create deployment group for an organization.
 *
 * Idempotent operation that creates a deployment group on-demand, and a new
 * deployment token when the stored one has expired or expires within a day.
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

  // A row without a stored expiry predates the column, so its token's lifetime is unknown: renew it.
  if (
    metadata?.deploymentGroupId &&
    metadata.deploymentToken &&
    metadata.deploymentTokenExpiresAt &&
    metadata.deploymentTokenExpiresAt.getTime() - Date.now() > RENEW_BEFORE_MS
  ) {
    return {
      deploymentGroupId: metadata.deploymentGroupId,
      deploymentToken: metadata.deploymentToken,
    }
  }

  // Keep the existing deployment group; only its token needs renewing
  const result = metadata?.deploymentGroupId
    ? {
        deploymentGroupId: metadata.deploymentGroupId,
        ...(await createDeploymentToken(metadata.deploymentGroupId, organizationName)),
      }
    : await createDeploymentGroupForOrganization(
        organizationId,
        organizationName,
        organizationSlug,
      )

  // Store in database (upsert)
  if (metadata) {
    await db
      .update(organizationMetadata)
      .set({ ...result, updatedAt: new Date() })
      .where(eq(organizationMetadata.organizationId, organizationId))
  } else {
    await db.insert(organizationMetadata).values({
      id: `org_meta_${organizationId}`,
      organizationId,
      ...result,
      createdAt: new Date(),
      updatedAt: new Date(),
    })
  }

  return {
    deploymentGroupId: result.deploymentGroupId,
    deploymentToken: result.deploymentToken,
  }
}
