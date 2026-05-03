"use client"

import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { Skeleton } from "@/components/ui/skeleton"
import { useAgentInfo } from "@/lib/queries"
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconChevronRight,
  IconExternalLink,
  IconGitMerge,
  IconGitPullRequest,
  IconShieldCheck,
  IconSparkles,
  IconTrendingDown,
  IconTrendingUp,
} from "@tabler/icons-react"
import { useQuery } from "@tanstack/react-query"
import { formatDistanceToNow } from "date-fns"
import type { ClassifiedPRWithReview } from "github-agent-remote-agent"
import { useSearchParams } from "next/navigation"
import { useState } from "react"
import { match } from "ts-pattern"
import { AIReviewCard } from "./_components/ai-review-card"
import { EncryptionIndicator } from "./_components/encryption-indicator"

const sizeColors = {
  small:
    "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400 border-green-200 dark:border-green-800",
  medium:
    "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800",
  large:
    "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400 border-red-200 dark:border-red-800",
} as const

const riskColors = {
  low: "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400 border-green-200 dark:border-green-800",
  medium:
    "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800",
  high: "bg-orange-100 text-orange-800 dark:bg-orange-900/30 dark:text-orange-400 border-orange-200 dark:border-orange-800",
  critical:
    "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400 border-red-200 dark:border-red-800",
} as const

function PRRow({ pr }: { pr: ClassifiedPRWithReview }) {
  const [isOpen, setIsOpen] = useState(false)

  return (
    <>
      <Collapsible open={isOpen} onOpenChange={setIsOpen}>
        <div className="rounded-lg border bg-card hover:bg-accent/50 transition-colors">
          <CollapsibleTrigger className="w-full p-4" asChild>
            <button className="flex items-center gap-4 text-left">
              <div className="shrink-0 font-mono font-medium text-muted-foreground w-12">
                #{pr.number}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-1">
                  {pr.state === "closed" && pr.mergedAt ? (
                    <IconGitMerge className="h-4 w-4 text-purple-500 shrink-0" />
                  ) : (
                    <IconGitPullRequest className="h-4 w-4 text-green-500 shrink-0" />
                  )}
                  <span className="font-medium truncate">{pr.title}</span>
                  {pr.aiReview && <IconSparkles className="h-4 w-4 text-purple-500 shrink-0" />}
                </div>
                <div className="flex items-center gap-2 flex-wrap text-xs text-muted-foreground">
                  <Badge
                    variant="outline"
                    className={
                      pr.mergedAt
                        ? "bg-purple-100 text-purple-800 dark:bg-purple-900/30 dark:text-purple-400 border-purple-200 dark:border-purple-800"
                        : pr.state === "open"
                          ? "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400 border-green-200 dark:border-green-800"
                          : ""
                    }
                  >
                    {pr.mergedAt ? "merged" : pr.state}
                  </Badge>
                  <Badge variant="outline" className={sizeColors[pr.size]}>
                    {pr.size}
                  </Badge>
                  <Badge variant="outline" className={riskColors[pr.risk]}>
                    {pr.risk}
                  </Badge>
                  <span className="font-mono">
                    <span className="text-green-600">+{pr.additions}</span>{" "}
                    <span className="text-red-600">-{pr.deletions}</span>
                  </span>
                  <span>•</span>
                  <span>{formatDistanceToNow(new Date(pr.createdAt), { addSuffix: true })}</span>
                </div>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <a
                  href={pr.url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-muted-foreground hover:text-foreground transition-colors"
                  onClick={e => e.stopPropagation()}
                >
                  <IconExternalLink className="h-4 w-4" />
                </a>
                <IconChevronRight
                  className={`h-4 w-4 text-muted-foreground transition-transform ${
                    isOpen ? "rotate-90" : ""
                  }`}
                />
              </div>
            </button>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="px-4 pb-4 pt-2 border-t">
              {pr.aiReview ? (
                <AIReviewCard review={pr.aiReview} />
              ) : (
                <div className="p-4 text-center text-muted-foreground text-sm">
                  No AI review available for this PR
                </div>
              )}
            </div>
          </CollapsibleContent>
        </div>
      </Collapsible>
    </>
  )
}

export default function PullRequestsPage() {
  const searchParams = useSearchParams()
  const integrationId = searchParams.get("integrationId")
  const agentId = searchParams.get("agentId")
  const repoName = searchParams.get("repo") || "Repository"

  const {
    data: agentInfo,
    isLoading: agentInfoLoading,
    error: agentInfoError,
  } = useAgentInfo(agentId || undefined)

  const {
    data: prs = [],
    isLoading: prsLoading,
    error: prsError,
  } = useQuery({
    queryKey: ["prs", integrationId, agentId],
    queryFn: async () => {
      if (!integrationId || !agentId) {
        throw new Error("Missing required parameters")
      }

      if (!agentInfo) {
        throw new Error("Agent info not available")
      }

      const rawAgentUrl = agentInfo.resources?.agent?.publicUrl
      if (!rawAgentUrl) {
        throw new Error("Agent is not running or doesn't have a public URL")
      }

      // The publicUrl flows from deployment state which the agent itself
      // writes via sync/reconcile. Validate it before using it in a fetch
      // so a malicious value can't redirect the browser to an arbitrary host.
      let agentOrigin: string
      try {
        const parsed = new URL(rawAgentUrl)
        if (parsed.protocol !== "https:") {
          throw new Error(`Agent URL must use https:// (got ${parsed.protocol})`)
        }
        agentOrigin = parsed.origin
      } catch {
        throw new Error("Agent URL is not a valid https:// URL")
      }

      const response = await fetch(
        `${agentOrigin}/prs?integrationId=${encodeURIComponent(integrationId)}`,
      )

      if (!response.ok) {
        throw new Error("Failed to fetch PRs from agent")
      }

      const data = await response.json()
      return (data.pullRequests || []) as ClassifiedPRWithReview[]
    },
    enabled: !!integrationId && !!agentId && !!agentInfo,
  })

  const loading = agentInfoLoading || prsLoading
  const error = agentInfoError || prsError

  const agentEnvironment = agentInfo?.resources?.agent?.publicUrl || "agent"

  const openPRs = prs.filter(pr => pr.state === "open").length
  const mergedPRs = prs.filter(pr => pr.mergedAt).length
  const highRiskPRs = prs.filter(pr => pr.risk === "high" || pr.risk === "critical").length
  const lowRiskPRs = prs.filter(pr => pr.risk === "low").length
  const largePRs = prs.filter(pr => pr.size === "large").length

  // Calculate percentages for badges
  const openPercentage = prs.length > 0 ? Math.round((openPRs / prs.length) * 100) : 0
  const highRiskPercentage = prs.length > 0 ? Math.round((highRiskPRs / prs.length) * 100) : 0
  const lowRiskPercentage = prs.length > 0 ? Math.round((lowRiskPRs / prs.length) * 100) : 0

  return (
    <div className="@container/main flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-bold tracking-tight">{repoName}</h1>
          <p className="text-muted-foreground">Pull requests with AI-powered code reviews</p>
        </div>
        {agentInfo && <EncryptionIndicator agentEnvironment={agentEnvironment} />}
      </div>

      {/* Metrics Summary */}
      {!loading && !error && prs.length > 0 && (
        <div className="*:data-[slot=card]:from-primary/5 *:data-[slot=card]:to-card dark:*:data-[slot=card]:bg-card grid grid-cols-1 gap-4 *:data-[slot=card]:bg-gradient-to-t *:data-[slot=card]:shadow-xs @xl/main:grid-cols-2 @5xl/main:grid-cols-4">
          <Card className="@container/card">
            <CardHeader>
              <CardDescription>Total PRs</CardDescription>
              <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
                {prs.length}
              </CardTitle>
              <CardAction>
                <Badge variant="outline" className="gap-1">
                  <IconGitPullRequest className="h-3 w-3" />
                  Analyzed
                </Badge>
              </CardAction>
            </CardHeader>
            <CardFooter className="flex-col items-start gap-1.5 text-sm">
              <div className="line-clamp-1 flex gap-2 font-medium">
                {openPRs > 0 ? `${openPRs} currently open` : "All closed"}{" "}
                <IconGitPullRequest className="size-4" />
              </div>
              <div className="text-muted-foreground">With AI-powered reviews</div>
            </CardFooter>
          </Card>

          <Card className="@container/card">
            <CardHeader>
              <CardDescription>Open PRs</CardDescription>
              <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
                {openPRs}
              </CardTitle>
              <CardAction>
                <Badge
                  variant="outline"
                  className={openPercentage > 30 ? "text-yellow-600" : "text-green-600"}
                >
                  {openPercentage > 30 ? (
                    <IconTrendingUp className="h-3 w-3" />
                  ) : (
                    <IconTrendingDown className="h-3 w-3" />
                  )}
                  {openPercentage}%
                </Badge>
              </CardAction>
            </CardHeader>
            <CardFooter className="flex-col items-start gap-1.5 text-sm">
              <div className="line-clamp-1 flex gap-2 font-medium">
                {openPercentage > 30 ? "Active development" : "Low backlog"}{" "}
                <IconGitMerge className="size-4" />
              </div>
              <div className="text-muted-foreground">{mergedPRs} merged successfully</div>
            </CardFooter>
          </Card>

          <Card className="@container/card">
            <CardHeader>
              <CardDescription>High Risk PRs</CardDescription>
              <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
                {highRiskPRs}
              </CardTitle>
              <CardAction>
                <Badge
                  variant="outline"
                  className={highRiskPRs > 0 ? "text-orange-600" : "text-green-600"}
                >
                  {highRiskPRs > 0 ? (
                    <IconAlertTriangle className="h-3 w-3" />
                  ) : (
                    <IconShieldCheck className="h-3 w-3" />
                  )}
                  {highRiskPercentage}%
                </Badge>
              </CardAction>
            </CardHeader>
            <CardFooter className="flex-col items-start gap-1.5 text-sm">
              <div className="line-clamp-1 flex gap-2 font-medium">
                {highRiskPRs > 0 ? "Needs attention" : "All clear"}{" "}
                <IconAlertTriangle className="size-4" />
              </div>
              <div className="text-muted-foreground">{lowRiskPRs} low risk PRs</div>
            </CardFooter>
          </Card>

          <Card className="@container/card">
            <CardHeader>
              <CardDescription>Large PRs</CardDescription>
              <CardTitle className="text-2xl font-semibold tabular-nums @[250px]/card:text-3xl">
                {largePRs}
              </CardTitle>
              <CardAction>
                <Badge
                  variant="outline"
                  className={largePRs > 2 ? "text-red-600" : "text-green-600"}
                >
                  {largePRs > 2 ? (
                    <IconTrendingUp className="h-3 w-3" />
                  ) : (
                    <IconTrendingDown className="h-3 w-3" />
                  )}
                  {largePRs > 0 ? "Review carefully" : "None"}
                </Badge>
              </CardAction>
            </CardHeader>
            <CardFooter className="flex-col items-start gap-1.5 text-sm">
              <div className="line-clamp-1 flex gap-2 font-medium">
                {largePRs > 2 ? "Consider splitting" : "Good sizing"}{" "}
                <IconSparkles className="size-4" />
              </div>
              <div className="text-muted-foreground">500+ lines of changes</div>
            </CardFooter>
          </Card>
        </div>
      )}

      {/* Pull Requests List */}
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-lg font-semibold">Pull Requests with AI Reviews</h2>
            <p className="text-sm text-muted-foreground">
              Click any PR to view detailed AI-powered code analysis
            </p>
          </div>
        </div>

        {loading ? (
          <div className="space-y-3">
            {[...Array(5)].map((_, i) => (
              <div key={i} className="rounded-lg border bg-card p-4">
                <div className="flex items-center gap-4">
                  <Skeleton className="h-6 w-12" />
                  <Skeleton className="h-6 flex-1" />
                  <Skeleton className="h-6 w-20" />
                  <Skeleton className="h-6 w-20" />
                </div>
              </div>
            ))}
          </div>
        ) : error ? (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-12 px-4">
              <div className="relative mb-6">
                <div className="absolute inset-0 bg-destructive/20 rounded-full blur-xl" />
                <div className="relative flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br from-destructive/10 to-destructive/5 ring-1 ring-destructive/20">
                  <IconAlertCircle className="h-8 w-8 text-destructive" />
                </div>
              </div>
              <h3 className="text-lg font-semibold mb-2">Failed to load PRs</h3>
              <p className="text-muted-foreground text-center max-w-md mb-2">
                {error instanceof Error ? error.message : "Unknown error"}
              </p>
              <p className="text-sm text-muted-foreground text-center mb-1">
                Make sure your agent is running
              </p>
              <p className="text-xs text-muted-foreground text-center">
                The browser connects directly to your agent for end-to-end encryption.
              </p>
            </CardContent>
          </Card>
        ) : prs.length === 0 ? (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-12 px-4">
              <div className="relative mb-6">
                <div className="absolute inset-0 bg-muted-foreground/10 rounded-full blur-xl" />
                <div className="relative flex h-16 w-16 items-center justify-center rounded-full bg-gradient-to-br from-muted to-muted/50 ring-1 ring-border">
                  <IconGitPullRequest className="h-8 w-8 text-muted-foreground" />
                </div>
              </div>
              <h3 className="text-lg font-semibold mb-2">No pull requests found</h3>
              <p className="text-muted-foreground">
                This repository doesn&apos;t have any pull requests yet.
              </p>
            </CardContent>
          </Card>
        ) : (
          <div className="space-y-3">
            {prs.map(pr => (
              <PRRow key={pr.number} pr={pr} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
