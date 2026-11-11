import { describe, expect, it } from "vitest"
import { AI } from "../ai.js"

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

  it("omits models unless declared, and keeps them in order with required defaulting to false", () => {
    expect(new AI("llm").build().config).not.toHaveProperty("models")
    const r = new AI("llm")
      .models([{ id: "claude-sonnet-4-6", required: true }, { id: "claude-haiku-4-5" }])
      .build()
    expect(r.config.models).toEqual([
      { id: "claude-sonnet-4-6", required: true },
      { id: "claude-haiku-4-5", required: false },
    ])
  })
})
