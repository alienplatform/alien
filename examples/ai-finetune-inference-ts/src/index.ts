import { ai, storage } from "@alienplatform/sdk"
import { Hono } from "hono"

// The public id the tuned model is served under (matches `servedModelId` in alien.ts).
const TUNED_MODEL = "support-tuned"
const TRAINING_KEY = "training.jsonl"

const app = new Hono()

// 1. Upload the JSONL training set into the customer's bucket. The tuning job
//    (submitted by the AI resource's controller at deploy time) reads it from
//    there — the data never leaves the customer's cloud. In a real app you'd
//    seed this before deploy; exposed here so the flow is runnable end-to-end.
app.post("/dataset", async c => {
  const body = await c.req.text()
  if (!body.trim()) {
    return c.json({ error: "POST JSONL training data as the request body" }, 400)
  }
  await storage("finetune-training-data").put(TRAINING_KEY, new TextEncoder().encode(body))
  const lines = body.split("\n").filter(l => l.trim()).length
  return c.json({ uploaded: TRAINING_KEY, examples: lines })
})

// 2. Fine-tune status. The tuned model shows up in the gateway's model list only
//    once its job has completed, so its presence is a simple readiness signal.
app.get("/finetune/status", async c => {
  const models = await ai("llm").getAvailableModels()
  const ready = models.some(m => m.id === TUNED_MODEL)
  return c.json({
    tunedModel: TUNED_MODEL,
    status: ready ? "ready" : "pending",
    availableModels: models.map(m => m.id),
  })
})

// 3. Inference against a base foundation model (the per-cloud catalog).
app.post("/chat", async c => {
  return chat(c, await defaultBaseModel())
})

// 4. Inference against the fine-tuned model — same OpenAI-compatible call, just a
//    different `model` id. The gateway routes it to the tuned artifact in-account.
app.post("/chat-tuned", async c => {
  return chat(c, TUNED_MODEL)
})

async function defaultBaseModel(): Promise<string> {
  const models = await ai("llm").getAvailableModels()
  const base = models.find(m => m.id !== TUNED_MODEL)?.id
  if (!base) throw new Error("no base models available for this cloud")
  return base
}

async function chat(c: Parameters<Parameters<typeof app.post>[1]>[0], model: string) {
  const { message } = await c.req.json<{ message?: string }>()
  if (!message) {
    return c.json({ error: "send { \"message\": \"...\" }" }, 400)
  }
  const completion = (await ai("llm").chat.completions.create({
    model,
    messages: [{ role: "user", content: message }],
  })) as { choices?: Array<{ message?: { content?: string } }> }
  return c.json({ model, answer: completion.choices?.[0]?.message?.content ?? "" })
}

export default app
