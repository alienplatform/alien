import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

// The local platform serves the AI gateway as a BYO-key provider; fine-tuning is
// a managed-cloud capability, so locally we verify the deployable surface —
// dataset upload, status shape, and (when a key is present) base inference — not
// a real tuning job. The cloud tuning flow is exercised by deploying to a cloud.
const OPENAI_KEY = process.env.OPENAI_API_KEY

describe("ai-finetune-inference-ts", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("uploads training data into the customer bucket", async () => {
    const jsonl = readFileSync(
      fileURLToPath(new URL("../sample-training.jsonl", import.meta.url)),
      "utf8",
    )
    const response = await fetch(`${deployment.url}/dataset`, {
      method: "POST",
      body: jsonl,
    })
    expect(response.status).toBe(200)
    const body = (await response.json()) as { uploaded: string; examples: number }
    expect(body.uploaded).toBe("training.jsonl")
    expect(body.examples).toBe(10)
  })

  it("rejects an empty dataset upload", async () => {
    const response = await fetch(`${deployment.url}/dataset`, { method: "POST", body: "" })
    expect(response.status).toBe(400)
  })

  it("reports a well-formed fine-tune status", async () => {
    const response = await fetch(`${deployment.url}/finetune/status`)
    expect(response.status).toBe(200)
    const body = (await response.json()) as {
      tunedModel: string
      status: string
      availableModels: string[]
    }
    expect(body.tunedModel).toBe("support-tuned")
    // Locally the managed tuning job never runs, so the tuned model is not ready.
    expect(body.status).toBe("pending")
    expect(Array.isArray(body.availableModels)).toBe(true)
  })

  it.skipIf(!OPENAI_KEY)("answers a base-model chat when a provider key is set", async () => {
    const response = await fetch(`${deployment.url}/chat`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ message: "Say the single word: pong" }),
    })
    expect(response.status).toBe(200)
    const body = (await response.json()) as { model: string; answer: string }
    expect(body.model).toBeTruthy()
    expect(body.answer.length).toBeGreaterThan(0)
  })
})
