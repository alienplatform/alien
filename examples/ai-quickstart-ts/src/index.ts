import { ai } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// The models this stack can call on the current cloud (curated per-cloud catalog).
app.get("/models", async c => {
  const models = await ai("assistant").getAvailableModels()
  return c.json({ models: models.map(m => m.id) })
})

// One-shot question -> answer. `?model=` overrides the default (first catalog entry).
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
