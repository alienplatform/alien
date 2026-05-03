import { command } from "@alienplatform/sdk"
import { applyLabels, classifyPullRequests, computeMetrics, fetchPullRequests } from "./github.js"
import { loadIntegrationConfig, saveIntegrationConfig } from "./integrations.js"
import type { IntegrationConfig, LabelResult } from "./types.js"

type SetIntegrationParams = {
  integrationId: string
  config: IntegrationConfig
}

type AnalyzeParams = {
  integrationId: string
}

type LabelParams = {
  integrationId: string
}

function normalizeConfig(config: IntegrationConfig): IntegrationConfig {
  const owner = config.owner?.trim()
  const repo = config.repo?.trim()

  if (!owner || !repo) {
    throw new Error("Integration config requires owner and repo")
  }

  return {
    owner,
    repo,
    token: config.token?.trim() || undefined,
    baseUrl: config.baseUrl?.trim() || undefined,
  }
}

function ensureIntegrationId(integrationId?: string): string {
  const id = integrationId?.trim()
  if (!id) {
    throw new Error("integrationId is required")
  }
  return id
}

export function registerCommands(): void {
  command<SetIntegrationParams, { ok: boolean }>(
    "set-integration",
    async ({ integrationId, config }) => {
      const id = ensureIntegrationId(integrationId)
      const normalized = normalizeConfig(config)
      console.log(`Saving integration config for ${id}: ${normalized.owner}/${normalized.repo}`)
      await saveIntegrationConfig(id, normalized)
      return { ok: true }
    },
  )

  command<AnalyzeParams, ReturnType<typeof computeMetrics>>(
    "analyze-repository",
    async ({ integrationId }) => {
      const id = ensureIntegrationId(integrationId)
      console.log(`Analyzing repository for integration ${id}`)
      const config = await loadIntegrationConfig(id)
      const prs = await fetchPullRequests(config)
      const classified = classifyPullRequests(prs)
      const metrics = computeMetrics(classified)
      console.log(
        `Analysis complete: ${metrics.totalPRs} PRs, review throughput score ${metrics.reviewThroughputScore}`,
      )
      return metrics
    },
  )

  command<LabelParams, LabelResult>("label-pull-requests", async ({ integrationId }) => {
    const id = ensureIntegrationId(integrationId)
    const config = await loadIntegrationConfig(id)

    if (!config.token || config.token === "demo") {
      console.log("Demo mode: returning mock label results")
      return {
        labeled: 6,
        labels: ["size:small", "size:medium", "size:large", "risk:high"],
      }
    }

    const prs = await fetchPullRequests(config)
    const openPrs = prs.filter(pr => pr.state === "open")
    const classified = classifyPullRequests(openPrs)

    for (const { pr, classification } of classified) {
      const labels = [`size:${classification.size}`, `risk:${classification.risk}`]
      console.log(`Applying labels to PR #${pr.number}: ${labels.join(", ")}`)
      await applyLabels(config, pr.number, labels)
    }

    console.log(`Labeled ${openPrs.length} open PRs`)
    return { labeled: openPrs.length }
  })
}
