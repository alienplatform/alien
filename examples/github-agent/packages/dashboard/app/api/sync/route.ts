import { start } from "workflow/api"
import { headers } from "next/headers"
import { auth } from "@/lib/auth"
import { syncIntegrationMetrics } from "@/workflows/sync-metrics"
import { db } from "@/lib/db"
import { integration, organizationMetadata } from "@/lib/schema"
import { alien, config } from "@/lib/config"
import { eq } from "drizzle-orm"

export async function POST(request: Request) {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session) {
    return Response.json({ error: "Unauthorized" }, { status: 401 })
  }

  const activeOrgId = session.session.activeOrganizationId
  if (!activeOrgId) {
    return Response.json({ error: "No active organization" }, { status: 400 })
  }

  const { integrationId, agentId: providedAgentId } = await request.json()

  if (!integrationId) {
    return Response.json(
      { error: "integrationId is required" },
      { status: 400 }
    )
  }

  // Verify integration belongs to organization
  const [integrationRecord] = await db
    .select()
    .from(integration)
    .where(eq(integration.id, integrationId))
    .limit(1)

  if (!integrationRecord || integrationRecord.organizationId !== activeOrgId) {
    return Response.json({ error: "Integration not found" }, { status: 404 })
  }

  // Use provided agent or discover first available agent
  let agentId = providedAgentId

  if (!agentId) {
    try {
      const [metadata] = await db
        .select()
        .from(organizationMetadata)
        .where(eq(organizationMetadata.organizationId, activeOrgId))
        .limit(1)

      if (metadata?.deploymentGroupId) {
        const result = await alien.deployments.list({
          workspace: config.workspace,
          deploymentGroup: metadata.deploymentGroupId,
        })
        if (result.items && result.items.length > 0) {
          agentId = result.items[0].id
        }
      }
    } catch (error) {
      console.error("Failed to list agents:", error)
    }
  }

  if (!agentId) {
    return Response.json(
      { error: "No agent available. Please deploy an agent first." },
      { status: 400 }
    )
  }

  // Start the workflow (non-blocking)
  await start(syncIntegrationMetrics, [integrationId, agentId])

  return Response.json({ success: true })
}
