import { headers } from "next/headers"
import { auth } from "@/lib/auth"
import { db } from "@/lib/db"
import { integration } from "@/lib/schema"
import { eq } from "drizzle-orm"
import { invokeCommand, type IntegrationConfig } from "@/lib/arc"

/**
 * PATCH endpoint to update an integration (e.g., reassign agent, mark as inactive)
 */
export async function PATCH(request: Request) {
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

  const { integrationId, agentId, isActive } = await request.json()

  if (!integrationId) {
    return Response.json({ error: "integrationId is required" }, { status: 400 })
  }

  // Verify the integration belongs to the current organization
  const existingIntegration = await db.query.integration.findFirst({
    where: eq(integration.id, integrationId),
  })

  if (!existingIntegration || existingIntegration.organizationId !== activeOrgId) {
    return Response.json({ error: "Integration not found" }, { status: 404 })
  }

  // Update the integration
  const updateData: any = {
    updatedAt: new Date(),
  }

  if (agentId !== undefined) {
    updateData.agentId = agentId
    // If we're assigning a new agent, mark as active
    if (agentId) {
      updateData.isActive = true
      
      // Reconfigure the new agent's vault with the integration credentials
      try {
        const config: IntegrationConfig = {
          owner: existingIntegration.owner,
          repo: existingIntegration.repo,
          token: existingIntegration.hasToken ? "existing" : "demo",
          baseUrl: existingIntegration.baseUrl || undefined,
        }
        
        await invokeCommand(agentId, "set-integration", {
          integrationId,
          config,
        })
      } catch (error) {
        console.error("Failed to reconfigure agent:", error)
        return Response.json(
          { error: "Failed to configure new agent. Make sure the agent is running." },
          { status: 500 }
        )
      }
    }
  }

  if (isActive !== undefined) {
    updateData.isActive = isActive
  }

  await db
    .update(integration)
    .set(updateData)
    .where(eq(integration.id, integrationId))

  return Response.json({ success: true })
}

