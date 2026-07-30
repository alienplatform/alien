import { ai } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// The models this deployment's cloud actually has enabled, each with an id,
// provider, and displayName for a picker. Models not enabled on this cloud are
// simply absent, so call this to discover what you can use.
app.get("/models", async c => {
  const models = await ai("assistant").getAvailableModels()
  return c.json({ models })
})

// One-shot question -> answer. Discover then pick: `?model=` overrides, else use the
// first model getAvailableModels returned for this cloud.
app.get("/ask", async c => {
  const question = c.req.query("q")
  if (!question) {
    return c.json({ error: "pass a question as ?q=..." }, 400)
  }
  const assistant = ai("assistant")
  const model = c.req.query("model") ?? (await assistant.getAvailableModels())[0]?.id
  if (!model) {
    return c.json({ error: "no models available for this cloud" }, 500)
  }
  const completion = await assistant.chat.completions.create({
    model,
    messages: [{ role: "user", content: question }],
  })
  return c.json({ model, answer: completion.choices[0]?.message?.content ?? "" })
})

export default app
