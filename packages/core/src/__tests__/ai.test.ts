import { describe, expect, it } from "vitest"
import { AI } from "../ai.js"
import { Storage } from "../storage.js"

describe("AI", () => {
  it("builds with just an id", () => {
    const r = new AI("llm").build()
    expect(r.config.type).toBe("ai")
    expect(r.config.id).toBe("llm")
  })

  // BYO-key external providers are NOT declared on the resource; they are
  // supplied at deploy time as an ExternalBinding::Ai. The resource carries
  // only its id, so there is no `external` field on the built config.
  it("does not carry an external field", () => {
    const r = new AI("llm").build()
    expect(r.config).not.toHaveProperty("external")
  })

  // A stray `external` config must not survive the schema parse, so it can never reach the resource.
  it("strips a stray external field at build time", () => {
    const ai = new AI("llm")
    ;(ai as unknown as { _config: Record<string, unknown> })._config.external = {
      provider: "openai",
    }
    const r = ai.build()
    expect(r.config).not.toHaveProperty("external")
    expect(r.config.id).toBe("llm")
  })

  it("omits finetune by default", () => {
    const r = new AI("llm").build()
    expect(r.config).not.toHaveProperty("finetune")
  })

  it("accepts a Storage resource for trainingData and resolves it to its id", () => {
    const dataset = new Storage("training-set").build()
    const r = new AI("llm")
      .finetune({ baseModel: "amazon.nova-lite-v1:0", trainingData: dataset })
      .build()
    const finetune = (r.config as { finetune?: Record<string, unknown> }).finetune
    expect(finetune).toBeDefined()
    expect(finetune?.baseModel).toBe("amazon.nova-lite-v1:0")
    expect(finetune?.trainingData).toBe("training-set")
  })

  it("accepts a string id for trainingData and carries all fields", () => {
    const r = new AI("llm")
      .finetune({
        baseModel: "amazon.nova-lite-v1:0",
        trainingData: "training-set",
        trainingKey: "data.jsonl",
        servedModelId: "finance-model",
        method: "lora",
      })
      .build()
    const finetune = (r.config as { finetune?: Record<string, unknown> }).finetune
    expect(finetune).toEqual({
      baseModel: "amazon.nova-lite-v1:0",
      trainingData: "training-set",
      trainingKey: "data.jsonl",
      servedModelId: "finance-model",
      method: "lora",
    })
  })

  it("is chainable and returns the same builder", () => {
    const ai = new AI("llm")
    expect(ai.finetune({ baseModel: "b", trainingData: "d" })).toBe(ai)
  })
})
