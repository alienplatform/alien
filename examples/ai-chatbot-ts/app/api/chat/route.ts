import { createOpenAICompatible } from "@ai-sdk/openai-compatible"
import { getAiConnection, getPostgresConnection } from "@alienplatform/sdk"
import { type UIMessage, convertToModelMessages, stepCountIs, streamText, tool } from "ai"
import { Client } from "pg"
import { z } from "zod"

// Streaming completions can outrun the default serverless request cap.
export const maxDuration = 30

// One shared connection per container, cached lazily, resolved lazily from the linked Postgres
// binding (ALIEN_DB_BINDING). getPostgresConnection reads the connection password
// from the cloud secret store at runtime using the workload's own identity, so no
// password is ever in the environment.
let dbConnection: Promise<Client> | undefined
function db(): Promise<Client> {
  if (!dbConnection) {
    dbConnection = (async () => {
      const conn = await getPostgresConnection("db")
      // Field style + conn.ssl, NOT conn.connectionString: node-postgres parses the
      // URL's sslmode and overrides ssl, which breaks the managed-cloud cert path.
      const client = new Client({
        host: conn.host,
        port: conn.port,
        database: conn.database,
        user: conn.username,
        password: conn.password,
        ssl: conn.ssl,
      })
      await client.connect()
      return client
    })().catch(err => {
      // Don't cache a failed connection; let the next request retry.
      dbConnection = undefined
      throw err
    })
  }
  return dbConnection
}

const queryDatabase = tool({
  description:
    "Run a read-only SQL query against the company's private Postgres database. " +
    "Tables: customers(id, name, plan, country, mrr_usd), orders(id, customer_id, amount_usd, status, created).",
  inputSchema: z.object({
    sql: z.string().describe("a single read-only SELECT statement for Postgres"),
  }),
  execute: async ({ sql }) => {
    // This tool exposes the database to a language model. node-postgres runs
    // semicolon-separated statements, so a bare "starts with SELECT" check is
    // bypassable (e.g. "select 1; delete ..."). Require a single statement that
    // starts with SELECT: strip a trailing ";", then reject any remaining ";".
    const statement = sql.trim().replace(/;\s*$/, "")
    if (!/^select\b/i.test(statement) || statement.includes(";")) {
      return { error: "only a single read-only SELECT statement is allowed" }
    }
    const client = await db()
    const result = await client.query(statement)
    return { rowCount: result.rowCount, rows: result.rows.slice(0, 50) }
  },
})

export async function POST(req: Request) {
  const { messages, model }: { messages: UIMessage[]; model?: string } = await req.json()

  // Resolve the AI binding at request time (the env is only populated in the running
  // workload, not at build). getAiConnection returns { baseURL, apiKey? }: a BYO-key
  // provider is called directly with the key; an ambient-cloud model routes through the
  // in-process gateway (started here on first use), which injects the credential. Same
  // code either way.
  const provider = createOpenAICompatible({ name: "alien", ...(await getAiConnection("llm")) })

  const result = streamText({
    model: provider(model || "gpt-4o-mini"),
    system:
      "You answer questions about the company's data. When a question needs data, write a " +
      "single read-only Postgres SELECT and call the queryDatabase tool, then summarize the " +
      "result for the user in plain English.",
    messages: await convertToModelMessages(messages),
    // Without a stop condition the model emits the tool call and never streams the
    // final answer after the tool result; this lets it loop tool-call -> result -> text.
    stopWhen: stepCountIs(6),
    tools: { queryDatabase },
  })

  return result.toUIMessageStreamResponse()
}
