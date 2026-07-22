import { AlienError } from "@alienplatform/core"
import { ai, parseAiBinding } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// GET /ai-test
//
// Proves that the runtime injected ALIEN_TEST_AI_BINDING and that the binding
// parses to a well-formed config. With `?invoke=1` it additionally lists the
// cloud's model catalog and makes a real one-line chat call through the
// gateway: the full app -> gateway -> cloud LLM path under the
// workload's ambient credentials.
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
    const model = models[0]?.id
    if (!model) {
      return c.json(
        { injected: true, service: config.service, locator, error: "model catalog is empty" },
        500,
      )
    }
    const completion = await binding.chat.completions.create({
      model,
      messages: [{ role: "user", content: "Reply with exactly one word: pong" }],
    })
    const reply = completion.choices[0]?.message?.content ?? ""
    return c.json({
      injected: true,
      service: config.service,
      locator,
      modelCount: models.length,
      model,
      reply,
    })
  } catch (error) {
    const alienErr = error instanceof AlienError ? error.toExternal() : { message: String(error) }
    return c.json({ injected: false, error: alienErr }, 500)
  }
})

export default app
