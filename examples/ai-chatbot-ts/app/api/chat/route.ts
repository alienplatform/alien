import { createAnthropic } from "@ai-sdk/anthropic"
import { createOpenAICompatible } from "@ai-sdk/openai-compatible"
import { type AiConnection, ai, getAiConnection, postgres } from "@alienplatform/sdk"
import { type UIMessage, convertToModelMessages, stepCountIs, streamText, tool } from "ai"
import { Pool } from "pg"
import { z } from "zod"
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

// The binding reads the password from the cloud secret store at runtime with the
// workload's own identity — it is never in the environment.
let dbPool: Promise<Pool> | undefined
function db(): Promise<Pool> {
  if (!dbPool) {
    dbPool = (async () => {
      const conn = await postgres("db").connection()
      // Field style + conn.ssl, NOT conn.connectionString: node-postgres parses the
      // URL's sslmode and overrides ssl, which breaks the managed-cloud cert path.
      return new Pool({
        host: conn.host,
        port: conn.port,
        database: conn.database,
        user: conn.username,
        password: conn.password,
        ssl: conn.ssl,
        // The model writes the SQL, so the session bounds it rather than the parser:
        // read-only stops writes, and the timeout stops a `pg_sleep` or runaway scan
        // from pinning a pool connection.
        options: "-c default_transaction_read_only=on -c statement_timeout=10000",
      })
    })().catch(err => {
      // Don't cache a failed resolution; let the next request retry.
      dbPool = undefined
      throw err
    })
  }
  return dbPool
}

const MAX_ROWS = 50

const queryDatabase = tool({
  description:
    "Run a read-only SQL query against the company's private Postgres database. " +
    "Tables: customers(id, name, plan, country, mrr_usd), orders(id, customer_id, amount_usd, status, created).",
  inputSchema: z.object({
    sql: z.string().describe("a single read-only SELECT or WITH statement for Postgres"),
  }),
  execute: async ({ sql }) => {
    // node-postgres runs semicolon-separated statements, so reject chained SQL here;
    // writes are stopped by the pool's read-only sessions, not by parsing.
    const statement = sql.trim().replace(/;\s*$/, "")
    if (!/^(select|with)\b/i.test(statement) || statement.includes(";")) {
      return { error: "only a single read-only SELECT or WITH statement is allowed" }
    }
    await ensureSeeded()
    const pool = await db()
    // A LIMIT keeps the database from returning rows the client would only buffer
    // and drop; the extra row is what tells the model its answer was truncated.
    const result = await pool.query(`select * from (${statement}) as q limit ${MAX_ROWS + 1}`)
    const rows = result.rows.slice(0, MAX_ROWS)
    return { rows, rowCount: rows.length, truncated: result.rows.length > MAX_ROWS }
  },
})

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
      "You answer questions about the company's data. When a question needs data, write a " +
      "single read-only Postgres SELECT and call the queryDatabase tool, then summarize the " +
      "result for the user in plain English.",
    messages: await convertToModelMessages(messages),
    // Without a stop condition the model never streams the answer after the tool result.
    stopWhen: stepCountIs(6),
    tools: { queryDatabase },
  })

  return result.toUIMessageStreamResponse()
}
