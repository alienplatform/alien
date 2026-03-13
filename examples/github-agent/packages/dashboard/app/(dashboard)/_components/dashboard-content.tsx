"use client"

import { useEffect, useState } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle, CardAction, CardFooter } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { formatDistanceToNow } from "date-fns"
import {
  IconGitPullRequest,
  IconClock,
  IconTrendingUp,
  IconTrendingDown,
  IconAlertTriangle,
  IconRefresh,
  IconActivity,
  IconGauge,
  IconFlame,
} from "@tabler/icons-react"
import { PrSizeChart } from "./pr-size-chart"
import { PrRiskChart } from "./pr-risk-chart"
import { MetricsHistoryChart } from "./metrics-history-chart"
import { DashboardSkeleton } from "./dashboard-skeleton"

interface DashboardContentProps {
  integrationId: string
  agentId: string
  repoName?: string
}

interface Metrics {
  totalPRs: number
  bySize: { small: number; medium: number; large: number }
  byRisk: { low: number; medium: number; high: number; critical: number }
  avgTimeToFirstReviewHours: number
  avgMergeTimeHours: number
  reviewThroughputScore: number
  churnHotspots: Array<{ file: string; changes: number }>
}

interface SyncStatus {
  lastSyncAt: Date | null
  lastSyncStatus: string | null
  lastSyncError: string | null
}

export function DashboardContent({ integrationId, agentId, repoName = "Repository" }: DashboardContentProps) {
  const [metrics, setMetrics] = useState<Metrics | null>(null)
  const [syncStatus, setSyncStatus] = useState<SyncStatus>({
    lastSyncAt: null,
    lastSyncStatus: null,
    lastSyncError: null,
  })
  const [loading, setLoading] = useState(true)
  const [syncing, setSyncing] = useState(false)

  const fetchMetrics = async () => {
    try {
      const response = await fetch(`/api/metrics?integrationId=${integrationId}`)
      if (response.ok) {
        const data = await response.json()
        if (data.metrics) {
          setMetrics(data.metrics)
        }
        if (data.syncStatus) {
          setSyncStatus({
            lastSyncAt: data.syncStatus.lastSyncAt ? new Date(data.syncStatus.lastSyncAt) : null,
            lastSyncStatus: data.syncStatus.lastSyncStatus,
            lastSyncError: data.syncStatus.lastSyncError,
          })
        }
      }
    } catch (error) {
      console.error("Failed to fetch metrics:", error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchMetrics()
    const interval = setInterval(fetchMetrics, 5000)
    return () => clearInterval(interval)
  }, [integrationId])

  const handleSync = async () => {
    setSyncing(true)
    try {
      await fetch("/api/sync", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ integrationId, agentId }),
      })
      await fetchMetrics()
    } catch (error) {
      console.error("Sync failed:", error)
    } finally {
      setSyncing(false)
    }
  }

  if (loading) {
    return <DashboardSkeleton />
  }

  if (!metrics) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center justify-center py-16">
          <div className="relative mb-6">
            <div className="absolute inset-0 bg-primary/20 rounded-full blur-xl animate-pulse" />
            <div className="relative flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br from-primary/10 to-primary/5 ring-1 ring-primary/20">
              <IconActivity className="h-8 w-8 text-primary" />
            </div>
          </div>
          <h3 className="text-lg font-semibold mb-2">Ready to analyze!</h3>
          <p className="text-muted-foreground text-center max-w-md mb-6">
            Click "Sync Now" to fetch metrics from your agent and start analyzing your repository.
          </p>
          <Button 
            onClick={handleSync} 
            disabled={syncing}
            className="animate-pulse hover:animate-none"
            size="lg"
          >
            <IconRefresh className={`mr-2 h-4 w-4 ${syncing ? "animate-spin" : ""}`} />
            {syncing ? "Syncing..." : "Sync Now"}
          </Button>
        </CardContent>
      </Card>
    )
  }

  const highRiskCount = metrics.byRisk.high + metrics.byRisk.critical

  return (
    <div className="space-y-6">
      {/* Sync status header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div className="flex items-center gap-2 flex-wrap">
          {syncStatus.lastSyncAt && (
            <Badge
              variant={syncStatus.lastSyncStatus === "success" ? "outline" : "destructive"}
              className="gap-1.5"
            >
              <IconClock className="h-3 w-3" />
              Last synced {formatDistanceToNow(syncStatus.lastSyncAt, { addSuffix: true })}
            </Badge>
          )}
          {syncStatus.lastSyncError && (
            <Badge variant="destructive" className="gap-1.5">
              <IconAlertTriangle className="h-3 w-3" />
              {syncStatus.lastSyncError}
            </Badge>
          )}
        </div>
        <Button variant="outline" size="sm" onClick={handleSync} disabled={syncing}>
          <IconRefresh className={`mr-2 h-4 w-4 ${syncing ? "animate-spin" : ""}`} />
          {syncing ? "Syncing..." : "Sync Now"}
        </Button>
      </div>

      {/* Stats cards */}
      <div className="*:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card dark:*:data-[slot=card]:bg-card grid grid-cols-1 gap-4 *:data-[slot=card]:bg-gradient-to-t *:data-[slot=card]:shadow-xs @xl/main:grid-cols-2 @5xl/main:grid-cols-4">
        <Card className="@container/card">
          <CardHeader>
            <CardDescription>Total PRs</CardDescription>
            <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
              {metrics.totalPRs}
            </CardTitle>
            <CardAction>
              <Badge variant="outline" className="gap-1">
                <IconTrendingUp className="h-3 w-3" />
                Analyzed
              </Badge>
            </CardAction>
          </CardHeader>
          <CardFooter className="flex-col items-start gap-1.5 text-sm">
            <div className="line-clamp-1 flex gap-2 font-medium">
              Pull requests analyzed <IconGitPullRequest className="size-4" />
            </div>
            <div className="text-muted-foreground">
              From connected repository
            </div>
          </CardFooter>
        </Card>

        <Card className="@container/card">
          <CardHeader>
            <CardDescription>Avg Review Time</CardDescription>
            <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
              {metrics.avgTimeToFirstReviewHours.toFixed(1)}h
            </CardTitle>
            <CardAction>
              <Badge variant="outline" className={metrics.avgTimeToFirstReviewHours < 4 ? "text-green-600" : "text-yellow-600"}>
                {metrics.avgTimeToFirstReviewHours < 4 ? (
                  <><IconTrendingDown className="h-3 w-3" /> Fast</>
                ) : (
                  <><IconTrendingUp className="h-3 w-3" /> Slow</>
                )}
              </Badge>
            </CardAction>
          </CardHeader>
          <CardFooter className="flex-col items-start gap-1.5 text-sm">
            <div className="line-clamp-1 flex gap-2 font-medium">
              Time to first review <IconClock className="size-4" />
            </div>
            <div className="text-muted-foreground">
              Average across all PRs
            </div>
          </CardFooter>
        </Card>

        <Card className="@container/card">
          <CardHeader>
            <CardDescription>Throughput Score</CardDescription>
            <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
              {metrics.reviewThroughputScore}/100
            </CardTitle>
            <CardAction>
              <Badge variant="outline" className={metrics.reviewThroughputScore >= 70 ? "text-green-600" : "text-yellow-600"}>
                {metrics.reviewThroughputScore >= 70 ? (
                  <><IconTrendingUp className="h-3 w-3" /> Good</>
                ) : (
                  <><IconTrendingDown className="h-3 w-3" /> Needs work</>
                )}
              </Badge>
            </CardAction>
          </CardHeader>
          <CardFooter className="flex-col items-start gap-1.5 text-sm">
            <div className="line-clamp-1 flex gap-2 font-medium">
              Review efficiency <IconGauge className="size-4" />
            </div>
            <div className="text-muted-foreground">
              Based on review patterns
            </div>
          </CardFooter>
        </Card>

        <Card className="@container/card">
          <CardHeader>
            <CardDescription>High Risk PRs</CardDescription>
            <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
              {highRiskCount}
            </CardTitle>
            <CardAction>
              <Badge variant="outline" className={highRiskCount === 0 ? "text-green-600" : "text-red-600"}>
                {highRiskCount === 0 ? (
                  <><IconTrendingDown className="h-3 w-3" /> Clear</>
                ) : (
                  <><IconAlertTriangle className="h-3 w-3" /> Attention</>
                )}
              </Badge>
            </CardAction>
          </CardHeader>
          <CardFooter className="flex-col items-start gap-1.5 text-sm">
            <div className="line-clamp-1 flex gap-2 font-medium">
              Requires attention <IconAlertTriangle className="size-4" />
            </div>
            <div className="text-muted-foreground">
              High + critical risk PRs
            </div>
          </CardFooter>
        </Card>
      </div>

      {/* Charts */}
      <div className="grid gap-4 md:grid-cols-2">
        <PrSizeChart 
          data={metrics.bySize} 
          integrationId={integrationId}
          agentId={agentId}
          repoName={repoName}
        />
        <PrRiskChart 
          data={metrics.byRisk}
          integrationId={integrationId}
          agentId={agentId}
          repoName={repoName}
        />
      </div>

      {/* Churn hotspots */}
      {metrics.churnHotspots && metrics.churnHotspots.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <IconFlame className="h-5 w-5 text-orange-500" />
              Churn Hotspots
            </CardTitle>
            <CardDescription>Files with the most changes across pull requests</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              {metrics.churnHotspots.slice(0, 5).map((hotspot, index) => (
                <div key={index} className="flex items-center justify-between p-3 rounded-lg bg-muted/50">
                  <div className="flex items-center gap-3 min-w-0">
                    <Badge variant="outline" className="tabular-nums font-mono shrink-0">
                      #{index + 1}
                    </Badge>
                    <code className="text-sm font-mono truncate">{hotspot.file}</code>
                  </div>
                  <Badge variant="secondary" className="ml-2 shrink-0 tabular-nums">
                    {hotspot.changes} changes
                  </Badge>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

