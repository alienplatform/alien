import { command, kv } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

// --- Webhook endpoints ---
// These receive callbacks from external services (GitHub, Stripe, Slack, etc.)
// The HTTPS URL is created automatically in the customer's cloud.
// Where it's reachable from depends on their network configuration.

app.post("/webhooks/:source", async c => {
  const source = c.req.param("source")
  const body = await c.req.json()
  const ev = await kv("events")

  const event = {
    source,
    body,
    headers: Object.fromEntries(c.req.raw.headers.entries()),
    receivedAt: new Date().toISOString(),
  }

  const key = `${source}:${Date.now()}`
  await ev.set(key, event)

  return c.json({ received: true, key })
})

app.get("/webhooks/:source/recent", async c => {
  const source = c.req.param("source")
  const ev = await kv("events")
  const results: { key: string; value: unknown }[] = []

  for await (const entry of ev.scan(`${source}:`)) {
    results.push({
      key: entry.key,
      value: JSON.parse(new TextDecoder().decode(entry.value)),
    })
    if (results.length >= 20) break
  }

  return c.json({ events: results, count: results.length })
})

app.get("/health", c => c.json({ status: "ok" }))

// --- Commands ---
// Query stored events from your control plane.

command("get-events", async ({ source, limit }: { source?: string; limit?: number }) => {
  const ev = await kv("events")
  const prefix = source ? `${source}:` : ""
  const results: { key: string; value: unknown }[] = []

  for await (const entry of ev.scan(prefix)) {
    results.push({
      key: entry.key,
      value: JSON.parse(new TextDecoder().decode(entry.value)),
    })
    if (limit && results.length >= limit) break
  }

  return { events: results, count: results.length }
})

command("get-stats", async ({ sources }: { sources: string[] }) => {
  const ev = await kv("events")
  const counts: Record<string, number> = {}

  for (const source of sources) {
    let count = 0
    for await (const _ of ev.scan(`${source}:`)) count++
    counts[source] = count
  }

  return { sources: counts }
})

export default app
