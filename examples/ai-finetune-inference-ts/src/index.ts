import { ai, storage } from "@alienplatform/sdk"
import { Hono } from "hono"

// The public id the tuned model is served under (matches `servedModelId` in alien.ts).
const TUNED_MODEL = "support-tuned"
const TRAINING_KEY = "training.jsonl"

const app = new Hono()

// 1. Upload the JSONL training set into the customer's bucket. Nothing is tuned yet —
//    the AI resource is already provisioned and Ready; fine-tuning is triggered at
//    runtime (step 2). The data never leaves the customer's cloud.
app.post("/dataset", async c => {
  const body = await c.req.text()
  if (!body.trim()) {
    return c.json({ error: "POST JSONL training data as the request body" }, 400)
  }
  await storage("finetune-training-data").put(TRAINING_KEY, new TextEncoder().encode(body))
  const lines = body.split("\n").filter(l => l.trim()).length
  return c.json({ uploaded: TRAINING_KEY, examples: lines })
})

// 2. Trigger fine-tuning at runtime. This is the whole point: the provisioned `llm`
//    resource is always Ready, and the app itself kicks off a job by calling the
//    gateway — `ai("llm").finetune(...)`. The gateway submits the cloud tuning job
//    (Bedrock / Vertex / Foundry) under the workload's ambient identity and returns a
//    job id to poll. Long-running (minutes to hours); this returns immediately.
app.post("/finetune", async c => {
  const { jobId, servedModel } = await ai("llm").finetune({ trainingKey: TRAINING_KEY })
  return c.json({ jobId, servedModel, message: "tuning started; poll /finetune/status?jobId=" })
})

// 3. Poll a job. The gateway queries the cloud live (stateless — no job state stored).
app.get("/finetune/status", async c => {
  const jobId = c.req.query("jobId")
  if (!jobId) {
    return c.json({ error: "pass ?jobId= from the POST /finetune response" }, 400)
  }
  const state = await ai("llm").finetuneStatus(jobId)
  return c.json(state)
})

// 4. Inference against a base foundation model (the per-cloud catalog).
app.post("/chat", async c => {
  return chat(c, await defaultBaseModel())
})

// 5. Inference against the fine-tuned model — same OpenAI-compatible call, just a
//    different `model` id. Works once the job has succeeded: the gateway rediscovers
//    the completed tuned model by convention and routes to it.
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
