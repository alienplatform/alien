import { command as arcCommand } from "@alienplatform/bindings"
import { applyLabels, classifyPullRequests, computeMetrics, fetchPullRequests } from "./github.js"
import { loadIntegrationConfig, saveIntegrationConfig } from "./integrations.js"
import type { IntegrationConfig, LabelResult } from "./types.js"

const command = arcCommand as unknown as <TParams, TResponse>(
  name: string,
  handler: (params: TParams) => Promise<TResponse> | TResponse,
) => void

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
      await saveIntegrationConfig(id, normalized)
      return { ok: true }
    },
  )

  command<AnalyzeParams, ReturnType<typeof computeMetrics>>(
    "analyze-repository",
    async ({ integrationId }) => {
      const id = ensureIntegrationId(integrationId)
      const config = await loadIntegrationConfig(id)
      const prs = await fetchPullRequests(config)
      const classified = classifyPullRequests(prs)
      return computeMetrics(classified)
    },
  )

  command<LabelParams, LabelResult>("label-pull-requests", async ({ integrationId }) => {
    const id = ensureIntegrationId(integrationId)
    const config = await loadIntegrationConfig(id)

    if (!config.token || config.token === "demo") {
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
      await applyLabels(config, pr.number, labels)
    }

    return { labeled: openPrs.length }
  })
}
