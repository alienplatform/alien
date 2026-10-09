import { auth } from "@/lib/auth"
import { alien, config } from "@/lib/config"
import { db } from "@/lib/db"
import { organizationMetadata } from "@/lib/schema"
import { eq } from "drizzle-orm"
import { headers } from "next/headers"

export async function GET() {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session) {
    return Response.json({ error: "Unauthorized" }, { status: 401 })
  }

  const activeOrgId = session.session.activeOrganizationId
  if (!activeOrgId) {
    return Response.json({ agents: [] })
  }

  try {
    // Get deployment group for organization
    const [metadata] = await db
      .select()
      .from(organizationMetadata)
      .where(eq(organizationMetadata.organizationId, activeOrgId))
      .limit(1)

    if (!metadata?.deploymentGroupId) {
      return Response.json({ agents: [] })
    }

    // List agents in deployment group
    const result = await alien.deployments.list({
      workspace: config.workspace,
      deploymentGroup: metadata.deploymentGroupId,
    })

    return Response.json({
      agents: (result.items || []).map(agent => ({
        id: agent.id || "unknown",
        name: agent.name || agent.id || "unknown",
        status: agent.status || "unknown",
        platform: agent.platform || "unknown",
      })),
    })
  } catch (error) {
    console.error("Failed to list agents:", error)
    return Response.json({ agents: [] })
  }
}
