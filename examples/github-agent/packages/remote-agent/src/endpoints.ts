import type { Hono } from "hono"
import { generateAIReview } from "./ai-reviews.js"
import { classifyPullRequests, fetchPullRequests } from "./github.js"
import { loadIntegrationConfig } from "./integrations.js"

export function registerEndpoints(app: Hono): void {
  app.get("/health", c => {
    console.log("health")
    return c.json({
      status: "ok",
      timestamp: new Date().toISOString(),
    })
  })

  app.get("/prs", async c => {
    const integrationId = c.req.query("integrationId")?.trim()

    if (!integrationId) {
      return c.json({ error: "integrationId is required" }, 400)
    }

    const config = await loadIntegrationConfig(integrationId)
    const prs = await fetchPullRequests(config)
    const classified = classifyPullRequests(prs)

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
        // Include AI review for each PR
        aiReview: generateAIReview(pr),
      })),
    })
  })
}
