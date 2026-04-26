import { auth } from "@/lib/auth"
import { db } from "@/lib/db"
import { metricsHistory, syncStatus } from "@/lib/schema"
import { desc, eq } from "drizzle-orm"
import { headers } from "next/headers"

export async function GET(request: Request) {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session) {
    return Response.json({ error: "Unauthorized" }, { status: 401 })
  }

  const { searchParams } = new URL(request.url)
  const integrationId = searchParams.get("integrationId")

  if (!integrationId) {
    return Response.json({ error: "integrationId is required" }, { status: 400 })
  }

  // Get the latest metrics
  const latestMetrics = await db.query.metricsHistory.findFirst({
    where: eq(metricsHistory.integrationId, integrationId),
    orderBy: [desc(metricsHistory.syncedAt)],
  })

  // Get sync status
  const sync = await db.query.syncStatus.findFirst({
    where: eq(syncStatus.integrationId, integrationId),
  })

  if (!latestMetrics) {
    return Response.json({
      metrics: null,
      syncStatus: sync,
    })
  }

  const metrics = {
    totalPRs: latestMetrics.totalPRs,
    bySize: {
      small: latestMetrics.smallPRs,
      medium: latestMetrics.mediumPRs,
      large: latestMetrics.largePRs,
    },
    byRisk: {
      low: latestMetrics.lowRiskPRs,
      medium: latestMetrics.mediumRiskPRs,
      high: latestMetrics.highRiskPRs,
      critical: latestMetrics.criticalRiskPRs,
    },
    avgTimeToFirstReviewHours: latestMetrics.avgTimeToFirstReviewHours || 0,
    avgMergeTimeHours: latestMetrics.avgMergeTimeHours || 0,
    reviewThroughputScore: latestMetrics.reviewThroughputScore || 0,
    churnHotspots: latestMetrics.churnHotspots ? JSON.parse(latestMetrics.churnHotspots) : [],
  }

  return Response.json({
    metrics,
    syncStatus: sync,
  })
}
