import { createHash } from "node:crypto"
import { command, kv, vault } from "@alienplatform/sdk"

// --- Sample data ---
// In production, replace this with a real database client (pg, mysql2, etc.).
// The connection string comes from the customer's vault — you never see it.

const SAMPLE_DATA: Record<string, any[]> = {
  users: [
    { id: 1, name: "Alice", email: "alice@example.com", role: "admin" },
    { id: 2, name: "Bob", email: "bob@example.com", role: "user" },
    { id: 3, name: "Charlie", email: "charlie@example.com", role: "user" },
  ],
  orders: [
    { id: 101, user_id: 1, total: 99.99, status: "shipped" },
    { id: 102, user_id: 2, total: 149.5, status: "pending" },
    { id: 103, user_id: 3, total: 29.0, status: "delivered" },
  ],
}

async function getConnectionConfig() {
  const v = await vault("credentials")
  const raw = await v.get("database")
  return JSON.parse(raw)
}

function simulateQuery(sql: string) {
  const lower = sql.toLowerCase()
  for (const [table, rows] of Object.entries(SAMPLE_DATA)) {
    if (lower.includes(`from ${table}`)) {
      return { rows, rowCount: rows.length }
    }
  }
  return { rows: [], rowCount: 0 }
}

// --- Commands ---

command("test-connection", async () => {
  const config = await getConnectionConfig()
  return {
    connected: true,
    database: config.database,
    host: config.host,
    // password is never returned — it stays in the customer's cloud
  }
})

command("query", async ({ sql, useCache }: { sql: string; useCache?: boolean }) => {
  const c = await kv("cache")
  const cacheKey = `query:${createHash("sha256").update(sql).digest("hex").slice(0, 16)}`

  if (useCache) {
    const cached = await c.get(cacheKey)
    if (cached) {
      return { ...JSON.parse(new TextDecoder().decode(cached)), cached: true }
    }
  }

  // In production, use the connection config to connect to the actual database
  await getConnectionConfig()
  const result = simulateQuery(sql)

  if (useCache) {
    await c.set(cacheKey, JSON.stringify(result))
  }

  return { ...result, cached: false }
})

command("list-tables", async () => {
  return { tables: Object.keys(SAMPLE_DATA) }
})
