import { auth } from "@/lib/auth"
import { alien, config } from "@/lib/config"
import { db } from "@/lib/db"
import { integration, organizationMetadata } from "@/lib/schema"
import { eq } from "drizzle-orm"
import { headers } from "next/headers"
import { Suspense } from "react"
import { DashboardContent } from "./_components/dashboard-content"
import { DashboardSkeleton } from "./_components/dashboard-skeleton"
import { EmptyDashboard } from "./_components/empty-dashboard"

export default async function DashboardPage() {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session?.user) {
    return null
  }

  const activeOrgId = session.session.activeOrganizationId
  if (!activeOrgId) {
    return null
  }

  const integrations = await db.query.integration.findMany({
    where: eq(integration.organizationId, activeOrgId),
    orderBy: (integration, { desc }) => [desc(integration.createdAt)],
  })

  // Get agents to check if any are available
  let hasAgents = false
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
      hasAgents = (result.items && result.items.length > 0) || false
    }
  } catch (error) {
    console.error("Failed to list agents:", error)
  }

  if (integrations.length === 0) {
    return <EmptyDashboard hasAgents={hasAgents} />
  }

  // Get first agent from deployment group
  let agentId: string | undefined
  if (hasAgents) {
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

  const firstIntegration = integrations[0]

  return (
    <div className="@container/main flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
      <div className="flex flex-col gap-1">
        <h1 className="text-2xl font-bold tracking-tight">Dashboard</h1>
        <p className="text-muted-foreground">
          Analytics for{" "}
          <span className="font-medium text-foreground">
            {firstIntegration.owner}/{firstIntegration.repo}
          </span>
        </p>
      </div>
      <Suspense fallback={<DashboardSkeleton />}>
        <DashboardContent
          integrationId={firstIntegration.id}
          agentId={agentId || ""}
          repoName={`${firstIntegration.owner}/${firstIntegration.repo}`}
        />
      </Suspense>
    </div>
  )
}
