"use workflow"

import { sleep } from "workflow"

type AnalysisMetrics = {
  totalPRs: number
  bySize: { small: number; medium: number; large: number }
  byRisk: { low: number; medium: number; high: number; critical: number }
  avgTimeToFirstReviewHours: number
  avgMergeTimeHours: number
  reviewThroughputScore: number
  churnHotspots: Array<{ file: string; changes: number }>
}

async function fetchMetricsFromAgent(integrationId: string, agentId: string) {
  "use step"
  
  const { invokeCommand } = await import("@/lib/arc")
  
  try {
    const metrics = await invokeCommand<AnalysisMetrics>(
      agentId,
      "analyze-repository",
      { integrationId }
    )
    
    return { success: true as const, metrics }
  } catch (error) {
    return { 
      success: false as const, 
      error: error instanceof Error ? error.message : "Unknown error" 
    }
  }
}

async function saveMetricsToDB(integrationId: string, metrics: AnalysisMetrics) {
  "use step"
  
  const { db } = await import("@/lib/db")
  const { metricsHistory, syncStatus } = await import("@/lib/schema")
  const { eq } = await import("drizzle-orm")
  
  const now = new Date()
  
  // Save metrics history
  await db.insert(metricsHistory).values({
    id: `metrics_${integrationId}_${now.getTime()}`,
    integrationId,
    totalPRs: metrics.totalPRs,
    smallPRs: metrics.bySize.small,
    mediumPRs: metrics.bySize.medium,
    largePRs: metrics.bySize.large,
    lowRiskPRs: metrics.byRisk.low,
    mediumRiskPRs: metrics.byRisk.medium,
    highRiskPRs: metrics.byRisk.high,
    criticalRiskPRs: metrics.byRisk.critical,
    avgTimeToFirstReviewHours: metrics.avgTimeToFirstReviewHours,
    avgMergeTimeHours: metrics.avgMergeTimeHours,
    reviewThroughputScore: metrics.reviewThroughputScore,
    churnHotspots: JSON.stringify(metrics.churnHotspots),
    syncedAt: now,
  })
  
  // Update sync status
  const existingSync = await db.query.syncStatus.findFirst({
    where: eq(syncStatus.integrationId, integrationId),
  })
  
  if (existingSync) {
    await db
      .update(syncStatus)
      .set({
        lastSyncAt: now,
        lastSyncStatus: "success",
        lastSyncError: null,
        nextSyncAt: new Date(now.getTime() + 5000), // 5 seconds from now
      })
      .where(eq(syncStatus.id, existingSync.id))
  } else {
    await db.insert(syncStatus).values({
      id: `sync_${integrationId}`,
      integrationId,
      lastSyncAt: now,
      lastSyncStatus: "success",
      lastSyncError: null,
      nextSyncAt: new Date(now.getTime() + 5000),
    })
  }
}

async function updateSyncError(integrationId: string, error: string) {
  "use step"
  
  const { db } = await import("@/lib/db")
  const { syncStatus } = await import("@/lib/schema")
  const { eq } = await import("drizzle-orm")
  
  const now = new Date()
  const existingSync = await db.query.syncStatus.findFirst({
    where: eq(syncStatus.integrationId, integrationId),
  })
  
  if (existingSync) {
    await db
      .update(syncStatus)
      .set({
        lastSyncAt: now,
        lastSyncStatus: "error",
        lastSyncError: error,
        nextSyncAt: new Date(now.getTime() + 5000),
      })
      .where(eq(syncStatus.id, existingSync.id))
  } else {
    await db.insert(syncStatus).values({
      id: `sync_${integrationId}`,
      integrationId,
      lastSyncAt: now,
      lastSyncStatus: "error",
      lastSyncError: error,
      nextSyncAt: new Date(now.getTime() + 5000),
    })
  }
}

async function getOrganizationIntegrations(organizationId: string) {
  "use step"
  
  const { db } = await import("@/lib/db")
  const { integration } = await import("@/lib/schema")
  const { eq } = await import("drizzle-orm")
  
  return await db.query.integration.findMany({
    where: eq(integration.organizationId, organizationId),
  })
}

export async function syncIntegrationMetrics(integrationId: string, agentId: string) {
  // Fetch metrics from the agent
  const result = await fetchMetricsFromAgent(integrationId, agentId)
  
  if (!result.success) {
    await updateSyncError(integrationId, result.error)
    return { success: false, error: result.error }
  }
  
  // Save to database
  await saveMetricsToDB(integrationId, result.metrics)
  
  return { success: true }
}

export async function syncAllIntegrationsLoop(organizationId: string, agentId: string) {
  while (true) {
    // Get all organization integrations
    const integrations = await getOrganizationIntegrations(organizationId)
    
    // Sync each integration
    for (const int of integrations) {
      await syncIntegrationMetrics(int.id, agentId)
    }
    
    // Wait 5 seconds before next sync
    await sleep("5s")
  }
}

