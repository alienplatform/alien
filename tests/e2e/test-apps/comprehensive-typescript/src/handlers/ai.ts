import { AlienError } from "@alienplatform/core"
import { ai, parseAiBinding } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// GET /ai-test
//
// Proves that the runtime injected ALIEN_TEST_AI_BINDING and that the binding
// parses to a well-formed config. With `?invoke=1` it additionally lists the
// the models advertised by the binding and invokes one cheap qualification model
// through the full app -> gateway -> cloud LLM path under the workload's ambient
// credentials. Catalog breadth is covered by deterministic protocol tests; a cloud
// E2E must not fan out paid requests across every catalog entry.
app.get("/ai-test", async c => {
  try {
    const binding = ai("test-ai")
    const config = await parseAiBinding("test-ai")
    if (!config) {
      return c.json({ injected: false, error: "ALIEN_TEST_AI_BINDING is not set" }, 500)
    }

    // Each ambient service names its scope differently; surface one locator string
    // so the e2e can assert the controller filled it in, whatever the cloud.
    const fields = config as Record<string, string | undefined>
    const locator =
      config.service === "bedrock"
        ? fields.region
        : config.service === "vertex"
          ? `${fields.project}/${fields.location}`
          : config.service === "foundry"
            ? fields.endpoint
            : undefined

    if (c.req.query("invoke") !== "1") {
      return c.json({ injected: true, service: config.service, locator })
    }

    const models = await binding.getAvailableModels()
    if (models.length === 0) {
      return c.json(
        { injected: true, service: config.service, locator, error: "no models available" },
        500,
      )
    }
    const preferredModel =
      config.service === "bedrock"
        ? "gpt-oss-20b"
        : config.service === "vertex"
          ? "gemini-2.5-flash"
          : "gpt-4.1"
    const probe = models.find(model => model.id === preferredModel) ?? models[0]
    let result: { model: string; ok: boolean; detail?: string }
    try {
      await binding.chat.completions.create({
        model: probe.id,
        max_completion_tokens: 1,
        messages: [{ role: "user", content: "ping" }],
      })
      result = { model: probe.id, ok: true }
    } catch (error) {
      // Classify on the live error instance: toExternal() sanitizes internal
      // errors down to a generic message, which would hide the 429 status.
      const ok = error instanceof AlienError && error.httpStatusCode === 429
      const detail = error instanceof AlienError ? JSON.stringify(error.toOptions()) : String(error)
      result = { model: probe.id, ok, detail }
    }
    return c.json({
      injected: true,
      service: config.service,
      locator,
      modelCount: models.length,
      models: models.map(m => ({ id: m.id, provider: m.provider, displayName: m.displayName })),
      probeModel: probe.id,
      results: [result],
    })
  } catch (error) {
    // toOptions(), not toExternal(): the harness reads this body to report why the
    // check failed, and sanitizing an internal error to "Internal server error"
    // leaves a cloud-only failure with no way to name itself.
    const alienErr = error instanceof AlienError ? error.toOptions() : { message: String(error) }
    return c.json({ injected: false, error: alienErr }, 500)
  }
})

export default app
