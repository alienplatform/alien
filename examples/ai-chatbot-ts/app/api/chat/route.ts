import { createAnthropic } from "@ai-sdk/anthropic"
import { createOpenAICompatible } from "@ai-sdk/openai-compatible"
import { type AiConnection, ai, getAiConnection } from "@alienplatform/sdk"
import { convertToModelMessages, stepCountIs, streamText, tool, type UIMessage } from "ai"
import { query } from "../../db"
import { type Ask, askSchema, plan, supportedFilters, unsupportedFilters } from "../../queries"
import { ensureSeeded } from "../../seed"

// The gateway forwards each model to its own upstream wire format rather than
// translating, so Claude needs the Anthropic client and everything else OpenAI.
// The catalog (alien-core's ai_catalog.rs) owns which is which.
function modelFor(modelId: string, connection: AiConnection) {
  if (modelId.startsWith("claude")) {
    const anthropic = createAnthropic({
      baseURL: connection.baseURL,
      // An ambient binding has no client key — the gateway signs with the workload's
      // own credential — and the empty string also keeps a stray ANTHROPIC_API_KEY in
      // the environment from being picked up and sent to it.
      apiKey: connection.apiKey ?? "",
    })
    return anthropic(modelId)
  }
  return createOpenAICompatible({ name: "alien", ...connection })(modelId)
}

const queryDatabase = tool({
  description:
    "Answer a question about the company's Postgres data. Pick the question that fits and " +
    "narrow it with the optional filters. Data: customers (name, plan, country, monthly " +
    "recurring revenue) and their orders (amount, status, date).",
  inputSchema: askSchema,
  execute: async (ask: Ask) => {
    const ignored = unsupportedFilters(ask)
    if (ignored.length > 0) {
      const takes = supportedFilters(ask.question)
      return {
        error: `${ask.question} does not take ${ignored.join(" or ")}; it takes ${
          takes.length > 0 ? takes.join(" and ") : "no filters"
        }`,
      }
    }
    await ensureSeeded()
    const { text, values } = plan(ask)
    const { rows } = await query(text, values)
    return { question: ask.question, rows, rowCount: rows.length }
  },
})

// Open on purpose: clicking the deployed URL and asking a question is the example. The cost is
// that anyone holding the URL spends the deployment's model quota, so a real app puts
// authentication and a per-caller rate limit here. README, "Deploying".
export async function POST(req: Request) {
  const { messages, model }: { messages: UIMessage[]; model?: string } = await req.json()

  // Model ids differ per cloud, so the fallback is the binding's first model, not a hardcoded id.
  const modelId = model || (await ai("llm").getAvailableModels())[0]?.id
  if (!modelId) {
    return Response.json({ error: "the AI binding exposes no models" }, { status: 503 })
  }

  // Resolved per request: the binding env exists only in the running workload, not at build.
  const connection = await getAiConnection("llm")

  const result = streamText({
    model: modelFor(modelId, connection),
    system:
      "You answer questions about the company's data. When a question needs data, call the " +
      "queryDatabase tool and summarize what comes back in plain English. If no question in " +
      "the tool covers what was asked, say what the data can and cannot answer.",
    messages: await convertToModelMessages(messages),
    // Without a stop condition the model never streams the answer after the tool result.
    stopWhen: stepCountIs(6),
    tools: { queryDatabase },
  })

  return result.toUIMessageStreamResponse()
}
