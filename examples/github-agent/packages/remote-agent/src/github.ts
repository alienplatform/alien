import type {
  AnalysisMetrics,
  ClassifiedPullRequest,
  IntegrationConfig,
  PullRequest,
  PullRequestClassification,
  PullRequestFile,
  PullRequestRisk,
  PullRequestSize,
} from "./types.js"

const MAX_PULL_REQUESTS = 30
const REQUEST_CONCURRENCY = 5

const DEMO_PULL_REQUESTS: PullRequest[] = [
  {
    number: 101,
    title: "Add authentication guard",
    state: "closed",
    url: "https://github.com/demo/acme-api/pull/101",
    createdAt: "2024-01-01T10:00:00Z",
    firstReviewAt: "2024-01-01T12:00:00Z",
    mergedAt: "2024-01-01T15:00:00Z",
    closedAt: "2024-01-01T15:05:00Z",
    additions: 48,
    deletions: 12,
    changedFiles: 3,
    files: [
      { path: "src/auth/login.ts", changes: 32 },
      { path: "src/auth/session.ts", changes: 18 },
      { path: "src/middleware.ts", changes: 10 },
    ],
  },
  {
    number: 102,
    title: "Refactor billing pipeline",
    state: "closed",
    url: "https://github.com/demo/acme-api/pull/102",
    createdAt: "2024-01-03T09:15:00Z",
    firstReviewAt: "2024-01-03T18:30:00Z",
    mergedAt: "2024-01-04T03:20:00Z",
    closedAt: "2024-01-04T03:25:00Z",
    additions: 180,
    deletions: 60,
    changedFiles: 7,
    files: [
      { path: "src/billing/ledger.ts", changes: 120 },
      { path: "src/billing/pricing.ts", changes: 55 },
      { path: "src/api/routes/billing.ts", changes: 32 },
    ],
  },
  {
    number: 103,
    title: "Introduce async ingestion workers",
    state: "closed",
    url: "https://github.com/demo/acme-api/pull/103",
    createdAt: "2024-01-07T14:40:00Z",
    firstReviewAt: "2024-01-08T22:10:00Z",
    mergedAt: "2024-01-10T08:05:00Z",
    closedAt: "2024-01-10T08:10:00Z",
    additions: 820,
    deletions: 290,
    changedFiles: 15,
    files: [
      { path: "src/workers/ingest.ts", changes: 380 },
      { path: "src/workers/queue.ts", changes: 260 },
      { path: "src/config/runtime.ts", changes: 90 },
      { path: "src/api/routes/ingest.ts", changes: 70 },
    ],
  },
  {
    number: 104,
    title: "Fix login redirect loop",
    state: "open",
    url: "https://github.com/demo/acme-api/pull/104",
    createdAt: "2024-01-12T11:10:00Z",
    additions: 18,
    deletions: 6,
    changedFiles: 2,
    files: [
      { path: "src/auth/login.ts", changes: 16 },
      { path: "src/auth/session.ts", changes: 8 },
    ],
  },
  {
    number: 105,
    title: "Add audit logging",
    state: "open",
    url: "https://github.com/demo/acme-api/pull/105",
    createdAt: "2024-01-14T08:00:00Z",
    firstReviewAt: "2024-01-14T17:30:00Z",
    additions: 260,
    deletions: 90,
    changedFiles: 9,
    files: [
      { path: "src/audit/logger.ts", changes: 110 },
      { path: "src/audit/writer.ts", changes: 80 },
      { path: "src/api/routes/audit.ts", changes: 40 },
    ],
  },
  {
    number: 106,
    title: "Clean up user onboarding copy",
    state: "closed",
    url: "https://github.com/demo/acme-api/pull/106",
    createdAt: "2024-01-17T07:20:00Z",
    firstReviewAt: "2024-01-17T09:00:00Z",
    mergedAt: "2024-01-17T12:10:00Z",
    closedAt: "2024-01-17T12:12:00Z",
    additions: 30,
    deletions: 14,
    changedFiles: 1,
    files: [{ path: "src/ui/onboarding.tsx", changes: 44 }],
  },
]

function isDemo(config: IntegrationConfig): boolean {
  return !config.token || config.token === "demo"
}

function getApiBase(config: IntegrationConfig): string {
  if (!config.baseUrl) {
    return "https://api.github.com"
  }

  const trimmed = config.baseUrl.replace(/\/+$/, "")
  if (trimmed.endsWith("/api/v3")) {
    return trimmed
  }

  return `${trimmed}/api/v3`
}

function hoursBetween(start: string, end: string): number {
  const startMs = new Date(start).getTime()
  const endMs = new Date(end).getTime()
  if (Number.isNaN(startMs) || Number.isNaN(endMs) || endMs <= startMs) {
    return 0
  }
  return (endMs - startMs) / 3_600_000
}

function average(values: number[]): number {
  if (values.length === 0) {
    return 0
  }
  const total = values.reduce((sum, value) => sum + value, 0)
  return total / values.length
}

async function fetchJson<T>(url: string, token?: string): Promise<T> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github+json",
  }

  if (token) {
    headers.Authorization = `token ${token}`
  }

  const response = await fetch(url, { headers })
  if (!response.ok) {
    const body = await response.text()
    throw new Error(`GitHub API error ${response.status} ${response.statusText}: ${body}`)
  }

  return (await response.json()) as T
}

async function mapWithConcurrency<T, R>(
  items: T[],
  concurrency: number,
  task: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results: R[] = new Array(items.length)
  let nextIndex = 0

  async function worker(): Promise<void> {
    while (nextIndex < items.length) {
      const currentIndex = nextIndex
      nextIndex += 1
      results[currentIndex] = await task(items[currentIndex]!, currentIndex)
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, items.length) }, () => worker())
  await Promise.all(workers)
  return results
}

async function fetchPullRequestFiles(
  apiBase: string,
  config: IntegrationConfig,
  prNumber: number,
): Promise<PullRequestFile[]> {
  type GitHubFile = { filename: string; changes: number }
  const url = `${apiBase}/repos/${config.owner}/${config.repo}/pulls/${prNumber}/files?per_page=100`
  const files = await fetchJson<GitHubFile[]>(url, config.token)
  return files.map(file => ({ path: file.filename, changes: file.changes ?? 0 }))
}

async function fetchPullRequestReviews(
  apiBase: string,
  config: IntegrationConfig,
  prNumber: number,
): Promise<string | undefined> {
  type GitHubReview = { submitted_at?: string | null }
  const url = `${apiBase}/repos/${config.owner}/${config.repo}/pulls/${prNumber}/reviews?per_page=100`
  const reviews = await fetchJson<GitHubReview[]>(url, config.token)
  const submittedTimes = reviews
    .map(review => review.submitted_at)
    .filter((value): value is string => Boolean(value))
    .sort()
  return submittedTimes[0]
}

async function fetchPullRequestDetails(
  apiBase: string,
  config: IntegrationConfig,
  prNumber: number,
): Promise<Pick<PullRequest, "additions" | "deletions" | "changedFiles">> {
  type GitHubDetails = {
    additions: number
    deletions: number
    changed_files: number
  }
  const url = `${apiBase}/repos/${config.owner}/${config.repo}/pulls/${prNumber}`
  const details = await fetchJson<GitHubDetails>(url, config.token)
  return {
    additions: details.additions ?? 0,
    deletions: details.deletions ?? 0,
    changedFiles: details.changed_files ?? 0,
  }
}

export async function fetchPullRequests(config: IntegrationConfig): Promise<PullRequest[]> {
  if (isDemo(config)) {
    return DEMO_PULL_REQUESTS
  }

  const apiBase = getApiBase(config)
  const listUrl = `${apiBase}/repos/${config.owner}/${config.repo}/pulls?state=all&per_page=${MAX_PULL_REQUESTS}`

  type GitHubPull = {
    number: number
    title: string
    state: "open" | "closed"
    html_url: string
    created_at: string
    merged_at?: string | null
    closed_at?: string | null
  }

  const pulls = await fetchJson<GitHubPull[]>(listUrl, config.token)

  return mapWithConcurrency(pulls, REQUEST_CONCURRENCY, async (pr) => {
    const [details, files, firstReviewAt] = await Promise.all([
      fetchPullRequestDetails(apiBase, config, pr.number),
      fetchPullRequestFiles(apiBase, config, pr.number),
      fetchPullRequestReviews(apiBase, config, pr.number),
    ])

    return {
      number: pr.number,
      title: pr.title,
      state: pr.state,
      url: pr.html_url,
      createdAt: pr.created_at,
      mergedAt: pr.merged_at ?? undefined,
      closedAt: pr.closed_at ?? undefined,
      additions: details.additions,
      deletions: details.deletions,
      changedFiles: details.changedFiles,
      files,
      firstReviewAt: firstReviewAt ?? undefined,
    }
  })
}

export function classifyPullRequest(pr: PullRequest): PullRequestClassification {
  const totalChanges = pr.additions + pr.deletions
  let size: PullRequestSize = "small"

  if (totalChanges >= 500) {
    size = "large"
  } else if (totalChanges >= 100) {
    size = "medium"
  }

  const reviewDelay = pr.firstReviewAt ? hoursBetween(pr.createdAt, pr.firstReviewAt) : 24
  const churnFactor = pr.changedFiles >= 12 ? 2 : pr.changedFiles >= 6 ? 1 : 0
  const sizeFactor = totalChanges >= 900 ? 3 : totalChanges >= 400 ? 2 : totalChanges >= 150 ? 1 : 0
  const reviewFactor = reviewDelay >= 24 ? 2 : reviewDelay >= 8 ? 1 : 0

  const riskScore = churnFactor + sizeFactor + reviewFactor

  let risk: PullRequestRisk = "low"
  if (riskScore >= 6) {
    risk = "critical"
  } else if (riskScore >= 4) {
    risk = "high"
  } else if (riskScore >= 2) {
    risk = "medium"
  }

  return { size, risk }
}

export function computeMetrics(classified: ClassifiedPullRequest[]): AnalysisMetrics {
  const bySize: AnalysisMetrics["bySize"] = {
    small: 0,
    medium: 0,
    large: 0,
  }
  const byRisk: AnalysisMetrics["byRisk"] = {
    low: 0,
    medium: 0,
    high: 0,
    critical: 0,
  }

  const churnMap = new Map<string, number>()
  const reviewTimes: number[] = []
  const mergeTimes: number[] = []

  for (const { pr, classification } of classified) {
    bySize[classification.size] += 1
    byRisk[classification.risk] += 1

    if (pr.firstReviewAt) {
      reviewTimes.push(hoursBetween(pr.createdAt, pr.firstReviewAt))
    }
    if (pr.mergedAt) {
      mergeTimes.push(hoursBetween(pr.createdAt, pr.mergedAt))
    }

    for (const file of pr.files) {
      const current = churnMap.get(file.path) ?? 0
      churnMap.set(file.path, current + file.changes)
    }
  }

  const avgReviewHours = average(reviewTimes)
  const avgMergeHours = average(mergeTimes)
  const reviewThroughputScore = Math.max(0, Math.min(100, Math.round(100 - avgReviewHours * 4)))

  const churnHotspots = Array.from(churnMap.entries())
    .map(([file, changes]) => ({ file, changes }))
    .sort((a, b) => b.changes - a.changes)
    .slice(0, 5)

  return {
    totalPRs: classified.length,
    bySize,
    byRisk,
    avgTimeToFirstReviewHours: Number(avgReviewHours.toFixed(2)),
    avgMergeTimeHours: Number(avgMergeHours.toFixed(2)),
    reviewThroughputScore,
    churnHotspots,
  }
}

export function classifyPullRequests(prs: PullRequest[]): ClassifiedPullRequest[] {
  return prs.map(pr => ({ pr, classification: classifyPullRequest(pr) }))
}

export async function applyLabels(
  config: IntegrationConfig,
  prNumber: number,
  labels: string[],
): Promise<void> {
  if (isDemo(config)) {
    return
  }

  const apiBase = getApiBase(config)
  const url = `${apiBase}/repos/${config.owner}/${config.repo}/issues/${prNumber}/labels`
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `token ${config.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ labels }),
  })

  if (!response.ok) {
    const body = await response.text()
    throw new Error(`GitHub label error ${response.status} ${response.statusText}: ${body}`)
  }
}
