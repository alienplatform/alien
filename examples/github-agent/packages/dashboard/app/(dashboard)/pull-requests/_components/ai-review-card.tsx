"use client"

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconCheck,
  IconCode,
  IconFileCode,
  IconInfoCircle,
  IconSparkles,
} from "@tabler/icons-react"
import type { AICodeExample, AIReview, AIReviewIssue } from "github-agent-remote-agent"
import { match } from "ts-pattern"

interface AIReviewCardProps {
  review: AIReview
}

export function AIReviewCard({ review }: AIReviewCardProps) {
  const getRatingColor = (rating: AIReview["overallRating"]) => {
    return match(rating)
      .with(
        "excellent",
        () =>
          "bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400 border-green-200 dark:border-green-800",
      )
      .with(
        "good",
        () =>
          "bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400 border-blue-200 dark:border-blue-800",
      )
      .with(
        "needs-work",
        () =>
          "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800",
      )
      .with(
        "concerning",
        () =>
          "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400 border-red-200 dark:border-red-800",
      )
      .exhaustive()
  }

  const getSeverityIcon = (severity: AIReviewIssue["severity"]) => {
    return match(severity)
      .with("critical", () => <IconAlertCircle className="h-4 w-4 text-red-500" />)
      .with("high", () => <IconAlertTriangle className="h-4 w-4 text-orange-500" />)
      .with("medium", () => <IconAlertTriangle className="h-4 w-4 text-yellow-500" />)
      .with("low", () => <IconInfoCircle className="h-4 w-4 text-blue-500" />)
      .with("info", () => <IconInfoCircle className="h-4 w-4 text-gray-500" />)
      .exhaustive()
  }

  const getSeverityColor = (severity: AIReviewIssue["severity"]) => {
    return match(severity)
      .with(
        "critical",
        () =>
          "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400 border-red-200 dark:border-red-800",
      )
      .with(
        "high",
        () =>
          "bg-orange-100 text-orange-800 dark:bg-orange-900/30 dark:text-orange-400 border-orange-200 dark:border-orange-800",
      )
      .with(
        "medium",
        () =>
          "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400 border-yellow-200 dark:border-yellow-800",
      )
      .with(
        "low",
        () =>
          "bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400 border-blue-200 dark:border-blue-800",
      )
      .with(
        "info",
        () =>
          "bg-gray-100 text-gray-800 dark:bg-gray-900/30 dark:text-gray-400 border-gray-200 dark:border-gray-800",
      )
      .exhaustive()
  }

  const getCategoryColor = (category: AIReviewIssue["category"]) => {
    return match(category)
      .with("security", () => "bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400")
      .with(
        "performance",
        () => "bg-orange-100 text-orange-800 dark:bg-orange-900/30 dark:text-orange-400",
      )
      .with(
        "maintainability",
        () => "bg-blue-100 text-blue-800 dark:bg-blue-900/30 dark:text-blue-400",
      )
      .with(
        "best-practice",
        () => "bg-purple-100 text-purple-800 dark:bg-purple-900/30 dark:text-purple-400",
      )
      .with(
        "bug-risk",
        () => "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400",
      )
      .exhaustive()
  }

  return (
    <Card className="border-purple-200 dark:border-purple-800 bg-gradient-to-br from-purple-50/50 to-transparent dark:from-purple-950/20">
      <CardHeader>
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div className="flex items-center gap-3">
            <div className="p-2 rounded-lg bg-purple-100 dark:bg-purple-900/30">
              <IconSparkles className="h-5 w-5 text-purple-600 dark:text-purple-400" />
            </div>
            <div>
              <CardTitle className="text-lg">AI Code Review</CardTitle>
              <CardDescription>
                Automated analysis with security & performance insights
              </CardDescription>
            </div>
          </div>
          <Badge variant="outline" className={getRatingColor(review.overallRating)}>
            {review.overallRating.replace("-", " ")}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* Summary */}
        <div className="rounded-lg bg-muted/50 p-4">
          <p className="text-sm text-foreground">{review.summary}</p>
        </div>

        {/* Issues */}
        {review.issues.length > 0 && (
          <div className="space-y-3">
            <h3 className="font-semibold text-sm flex items-center gap-2">
              <IconAlertCircle className="h-4 w-4" />
              Issues Found ({review.issues.length})
            </h3>
            <div className="space-y-2">
              {review.issues.map((issue: AIReviewIssue, idx: number) => (
                <div key={idx} className="rounded-lg border bg-card p-3 space-y-2">
                  <div className="flex items-start gap-2">
                    {getSeverityIcon(issue.severity)}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap mb-1">
                        <p className="font-medium text-sm">{issue.title}</p>
                        <Badge variant="outline" className={getSeverityColor(issue.severity)}>
                          {issue.severity}
                        </Badge>
                        <Badge variant="outline" className={getCategoryColor(issue.category)}>
                          {issue.category}
                        </Badge>
                      </div>
                      {issue.file && (
                        <div className="flex items-center gap-1 text-xs text-muted-foreground mb-2">
                          <IconFileCode className="h-3 w-3" />
                          <span className="font-mono">{issue.file}</span>
                          {issue.line && <span>:{issue.line}</span>}
                        </div>
                      )}
                      <p className="text-sm text-muted-foreground">{issue.description}</p>
                      {issue.suggestion && (
                        <div className="mt-2 rounded bg-green-50 dark:bg-green-950/20 border border-green-200 dark:border-green-800 p-2">
                          <p className="text-xs font-medium text-green-800 dark:text-green-400 mb-1">
                            💡 Suggestion:
                          </p>
                          <p className="text-xs text-green-700 dark:text-green-300 font-mono">
                            {issue.suggestion}
                          </p>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Highlights */}
        {review.highlights.length > 0 && (
          <div className="space-y-3">
            <h3 className="font-semibold text-sm flex items-center gap-2">
              <IconCheck className="h-4 w-4 text-green-500" />
              Highlights
            </h3>
            <ul className="space-y-2">
              {review.highlights.map((highlight: string, idx: number) => (
                <li key={idx} className="flex items-start gap-2 text-sm">
                  <IconCheck className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                  <span>{highlight}</span>
                </li>
              ))}
            </ul>
          </div>
        )}

        {/* Code Examples */}
        {review.codeExamples.length > 0 && (
          <div className="space-y-3">
            <h3 className="font-semibold text-sm flex items-center gap-2">
              <IconCode className="h-4 w-4" />
              Code Examples
            </h3>
            <Accordion type="single" collapsible className="w-full">
              {review.codeExamples.map((example: AICodeExample, idx: number) => (
                <AccordionItem key={idx} value={`example-${idx}`}>
                  <AccordionTrigger className="text-sm font-mono">
                    {example.file} (lines {example.lineStart}-{example.lineEnd})
                  </AccordionTrigger>
                  <AccordionContent>
                    <pre className="rounded-lg bg-muted p-4 overflow-x-auto text-xs">
                      <code>{example.code}</code>
                    </pre>
                  </AccordionContent>
                </AccordionItem>
              ))}
            </Accordion>
          </div>
        )}

        {/* Raw Analysis (the "sensitive" data) */}
        <div className="space-y-3">
          <h3 className="font-semibold text-sm flex items-center gap-2">
            <IconFileCode className="h-4 w-4" />
            Detailed Analysis
            <Badge variant="outline" className="ml-auto text-xs">
              Raw AI Output
            </Badge>
          </h3>
          <Accordion type="single" collapsible className="w-full">
            <AccordionItem value="raw-analysis">
              <AccordionTrigger className="text-sm">
                View full AI analysis (includes code snippets & detailed reasoning)
              </AccordionTrigger>
              <AccordionContent>
                <div className="rounded-lg bg-muted/50 p-4 border border-purple-200 dark:border-purple-800">
                  <div className="prose prose-sm dark:prose-invert max-w-none">
                    <pre className="whitespace-pre-wrap text-xs font-mono">
                      {review.rawAnalysis}
                    </pre>
                  </div>
                </div>
              </AccordionContent>
            </AccordionItem>
          </Accordion>
        </div>
      </CardContent>
    </Card>
  )
}
