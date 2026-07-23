import { AlienError } from "@alienplatform/core"
import { ai, parseAiBinding } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// GET /ai-test
//
// Proves that the runtime injected ALIEN_TEST_AI_BINDING and that the binding
// parses to a well-formed config. With `?invoke=1` it additionally lists the
// cloud's ENABLED models (getAvailableModels is availability-filtered) and invokes
// every one through the gateway: the full app -> gateway -> cloud LLM path under the
// workload's ambient credentials. Invoking each listed model is only sound because
// the list is filtered to what is actually enabled; a 403/404 on a listed model
// would mean the availability filter is broken.
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
    // Invoke every listed model with a one-token probe. `ok` is true on a real
    // completion or a 429 rate-limit (the model is enabled, the account is
    // quota-limited); anything else on a listed model is a filter bug.
    const results = await Promise.all(
      models.map(async model => {
        try {
          await binding.chat.completions.create({
            model: model.id,
            max_completion_tokens: 1,
            messages: [{ role: "user", content: "ping" }],
          })
          return { model: model.id, ok: true }
        } catch (error) {
          // Classify on the live error instance: toExternal() sanitizes internal
          // errors down to a generic message, which would hide the 429 status.
          // toOptions() keeps the real detail for this same-process diagnostic.
          const ok = error instanceof AlienError && error.httpStatusCode === 429
          const detail =
            error instanceof AlienError ? JSON.stringify(error.toOptions()) : String(error)
          return { model: model.id, ok, detail }
        }
      }),
    )
    return c.json({
      injected: true,
      service: config.service,
      locator,
      modelCount: models.length,
      models: models.map(m => ({ id: m.id, provider: m.provider, displayName: m.displayName })),
      results,
    })
  } catch (error) {
    const alienErr = error instanceof AlienError ? error.toExternal() : { message: String(error) }
    return c.json({ injected: false, error: alienErr }, 500)
  }
})

export default app
