import { listAgents } from "@/lib/alien"
import { type IntegrationConfig, invokeCommand } from "@/lib/arc"
import { auth } from "@/lib/auth"
import { db } from "@/lib/db"
import { integration } from "@/lib/schema"
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
    return Response.json({ error: "No active organization" }, { status: 400 })
  }

  const integrations = await db.query.integration.findMany({
    where: eq(integration.organizationId, activeOrgId),
    orderBy: (integration, { desc }) => [desc(integration.createdAt)],
  })

  // Check if any integration's agent no longer exists and mark as inactive
  try {
    const agents = await listAgents(activeOrgId)
    const agentIds = new Set(agents.map(a => a.id))

    for (const int of integrations) {
      if (int.agentId && int.isActive && !agentIds.has(int.agentId)) {
        // Agent no longer exists, mark integration as inactive
        await db
          .update(integration)
          .set({ isActive: false, updatedAt: new Date() })
          .where(eq(integration.id, int.id))
        // Update the local object for the response
        int.isActive = false
      }
    }
  } catch (error) {
    console.error("Failed to check agent status:", error)
  }

  return Response.json({ integrations })
}

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

  const { owner, repo, token, baseUrl, agentId: providedAgentId } = await request.json()

  if (!owner || !repo) {
    return Response.json({ error: "owner and repo are required" }, { status: 400 })
  }

  // Use provided agentId, env var, or find first available agent
  let agentId = providedAgentId || process.env.AGENT_ID

  if (!agentId) {
    // Try to find the first available agent in this organization's deployment group
    try {
      const agents = await listAgents(activeOrgId)
      if (agents.length > 0) {
        agentId = agents[0].id
      }
    } catch (error) {
      console.error("Failed to list agents:", error)
    }
  }

  if (!agentId) {
    return Response.json(
      { error: "No agent available. Please deploy an agent first." },
      { status: 400 },
    )
  }

  const integrationId = `github-${activeOrgId}-${owner}-${repo}`
    .toLowerCase()
    .replace(/[^a-z0-9-]/g, "-")

  // Check if integration already exists for this organization
  const existing = await db.query.integration.findFirst({
    where: eq(integration.id, integrationId),
  })

  if (existing) {
    return Response.json({ error: "Integration already exists" }, { status: 400 })
  }

  // Send credentials to agent's vault
  const config: IntegrationConfig = {
    owner,
    repo,
    token: token || "demo", // Use "demo" for demo mode
    baseUrl: baseUrl || undefined,
  }

  try {
    await invokeCommand(agentId, "set-integration", {
      integrationId,
      config,
    })
  } catch (error) {
    console.error("Failed to set integration in agent:", error)
    return Response.json(
      { error: "Failed to configure agent. Make sure the agent is running." },
      { status: 500 },
    )
  }

  // Store only metadata in database
  await db.insert(integration).values({
    id: integrationId,
    organizationId: activeOrgId,
    agentId: agentId,
    owner,
    repo,
    baseUrl: baseUrl || null,
    hasToken: !!token && token !== "demo",
    isActive: true,
    createdAt: new Date(),
    updatedAt: new Date(),
  })

  return Response.json({ success: true, integrationId })
}
