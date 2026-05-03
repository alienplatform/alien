import type { Hono } from "hono"
import { generateAIReview } from "./ai-reviews.js"
import { classifyPullRequests, fetchPullRequests } from "./github.js"
import { loadIntegrationConfig } from "./integrations.js"

export function registerEndpoints(app: Hono): void {
  app.get("/health", c => {
    return c.json({
      status: "ok",
      timestamp: new Date().toISOString(),
    })
  })

  app.get("/prs", async c => {
    const start = Date.now()
    const integrationId = c.req.query("integrationId")?.trim()

    if (!integrationId) {
      return c.json({ error: "integrationId is required" }, 400)
    }

    const config = await loadIntegrationConfig(integrationId)
    console.log(`Fetching PRs for integration ${integrationId}`)

    const prs = await fetchPullRequests(config)
    const classified = classifyPullRequests(prs)

    for (const { pr, classification } of classified) {
      console.log(
        `PR #${pr.number}: ${pr.additions}+${pr.deletions} changes, ${pr.changedFiles} files → size:${classification.size}, risk:${classification.risk}`,
      )
    }

    console.log(`GET /prs → 200 (${Date.now() - start}ms, ${classified.length} PRs)`)

    return c.json({
      integrationId,
      pullRequests: classified.map(({ pr, classification }) => ({
        number: pr.number,
        title: pr.title,
        state: pr.state,
        url: pr.url,
        createdAt: pr.createdAt,
        mergedAt: pr.mergedAt,
        additions: pr.additions,
        deletions: pr.deletions,
        changedFiles: pr.changedFiles,
        size: classification.size,
        risk: classification.risk,
        aiReview: generateAIReview(pr),
      })),
    })
  })
}
