import { z } from "zod"

// Integration configuration
export const IntegrationConfigSchema = z.object({
  owner: z.string(),
  repo: z.string(),
  token: z.string().optional(),
  baseUrl: z.string().optional(),
})

export type IntegrationConfig = z.infer<typeof IntegrationConfigSchema>

// Pull request classification
export const PullRequestSizeSchema = z.enum(["small", "medium", "large"])
export const PullRequestRiskSchema = z.enum(["low", "medium", "high", "critical"])

export const PullRequestFileSchema = z.object({
  path: z.string(),
  changes: z.number(),
})

export const PullRequestSchema = z.object({
  number: z.number(),
  title: z.string(),
  state: z.enum(["open", "closed"]),
  url: z.string(),
  createdAt: z.string(),
  mergedAt: z.string().optional(),
  closedAt: z.string().optional(),
  additions: z.number(),
  deletions: z.number(),
  changedFiles: z.number(),
  files: z.array(PullRequestFileSchema),
  firstReviewAt: z.string().optional(),
})

export const PullRequestClassificationSchema = z.object({
  size: PullRequestSizeSchema,
  risk: PullRequestRiskSchema,
})

export const ClassifiedPullRequestSchema = z.object({
  pr: PullRequestSchema,
  classification: PullRequestClassificationSchema,
})

// AI Review schemas
export const AIReviewSeveritySchema = z.enum(["critical", "high", "medium", "low", "info"])
export const AIReviewCategorySchema = z.enum([
  "security",
  "performance",
  "maintainability",
  "best-practice",
  "bug-risk",
])

export const AIReviewIssueSchema = z.object({
  severity: AIReviewSeveritySchema,
  category: AIReviewCategorySchema,
  title: z.string(),
  description: z.string(),
  file: z.string().optional(),
  line: z.number().optional(),
  suggestion: z.string().optional(),
})

export const AICodeExampleSchema = z.object({
  file: z.string(),
  lineStart: z.number(),
  lineEnd: z.number(),
  code: z.string(),
  language: z.string(),
})

export const AIReviewRatingSchema = z.enum(["excellent", "good", "needs-work", "concerning"])

export const AIReviewSchema = z.object({
  prNumber: z.number(),
  summary: z.string(),
  overallRating: AIReviewRatingSchema,
  issues: z.array(AIReviewIssueSchema),
  highlights: z.array(z.string()),
  codeExamples: z.array(AICodeExampleSchema),
  rawAnalysis: z.string(),
  reviewedAt: z.string(),
})

// Analysis metrics
export const AnalysisMetricsSchema = z.object({
  totalPRs: z.number(),
  bySize: z.object({
    small: z.number(),
    medium: z.number(),
    large: z.number(),
  }),
  byRisk: z.object({
    low: z.number(),
    medium: z.number(),
    high: z.number(),
    critical: z.number(),
  }),
  avgTimeToFirstReviewHours: z.number(),
  avgMergeTimeHours: z.number(),
  reviewThroughputScore: z.number(),
  churnHotspots: z.array(
    z.object({
      file: z.string(),
      changes: z.number(),
    }),
  ),
})

export const LabelResultSchema = z.object({
  labeled: z.number(),
  labels: z.array(z.string()).optional(),
})

// API response schemas
export const ClassifiedPRWithReviewSchema = z.object({
  number: z.number(),
  title: z.string(),
  state: z.string(),
  url: z.string(),
  createdAt: z.string(),
  mergedAt: z.string().optional(),
  additions: z.number(),
  deletions: z.number(),
  changedFiles: z.number(),
  size: PullRequestSizeSchema,
  risk: PullRequestRiskSchema,
  aiReview: AIReviewSchema,
})

// Inferred types
export type PullRequest = z.infer<typeof PullRequestSchema>
export type PullRequestFile = z.infer<typeof PullRequestFileSchema>
export type PullRequestSize = z.infer<typeof PullRequestSizeSchema>
export type PullRequestRisk = z.infer<typeof PullRequestRiskSchema>
export type PullRequestClassification = z.infer<typeof PullRequestClassificationSchema>
export type ClassifiedPullRequest = z.infer<typeof ClassifiedPullRequestSchema>
export type AIReviewIssue = z.infer<typeof AIReviewIssueSchema>
export type AICodeExample = z.infer<typeof AICodeExampleSchema>
export type AIReview = z.infer<typeof AIReviewSchema>
export type AnalysisMetrics = z.infer<typeof AnalysisMetricsSchema>
export type LabelResult = z.infer<typeof LabelResultSchema>
export type ClassifiedPRWithReview = z.infer<typeof ClassifiedPRWithReviewSchema>

