import {
  command,
  kv,
  onCronEvent,
  onQueueMessage,
  onStorageEvent,
  queue,
  waitUntil,
} from "@alienplatform/sdk"
import type { Kv } from "@alienplatform/sdk"
import { Hono } from "hono"

/** Iterate every key under a prefix, following the scan cursor across pages. */
async function* scanAll(store: Kv, prefix: string) {
  let cursor: string | undefined
  do {
    const page = await store.scan(prefix, undefined, cursor)
    for (const item of page.items) yield item
    cursor = page.nextCursor
  } while (cursor)
}

// --- Event Handlers ---

// Process messages from the inbox queue
onQueueMessage("*", async message => {
  const ev = kv("events")
  const payload =
    typeof message.payload === "string"
      ? message.payload
      : new TextDecoder().decode(message.payload as Uint8Array)

  await ev.setJson(`queue:${message.id}`, {
    type: "queue",
    source: message.source,
    payload,
    processedAt: new Date().toISOString(),
  })
})

// React to new files in storage
onStorageEvent("*", async event => {
  const ev = kv("events")
  const key = event.objectKey.replace(/\//g, "_")

  await ev.setJson(`storage:${key}`, {
    type: "storage",
    bucket: event.bucketName,
    objectKey: event.objectKey,
    eventType: event.eventType,
    size: event.size,
    processedAt: new Date().toISOString(),
  })
})

// Run on a schedule (every hour)
onCronEvent("*", async event => {
  const ev = kv("events")

  await ev.setJson(`cron:${Date.now()}`, {
    type: "cron",
    schedule: event.scheduleName,
    scheduledTime: event.timestamp,
    processedAt: new Date().toISOString(),
  })
})

// --- Commands for querying processed events ---

command("get-events", async ({ type, limit }: { type?: string; limit?: number }) => {
  const ev = kv("events")
  const prefix = type ? `${type}:` : ""
  const results: { key: string; value: unknown }[] = []

  for await (const entry of scanAll(ev, prefix)) {
    results.push({
      key: entry.key,
      value: JSON.parse(new TextDecoder().decode(entry.value)),
    })
    if (limit && results.length >= limit) break
  }

  return { events: results, count: results.length }
})

command("get-stats", async () => {
  const ev = kv("events")
  let queueCount = 0
  let storageCount = 0
  let cronCount = 0

  for await (const _ of scanAll(ev, "queue:")) queueCount++
  for await (const _ of scanAll(ev, "storage:")) storageCount++
  for await (const _ of scanAll(ev, "cron:")) cronCount++

  return { queue: queueCount, storage: storageCount, cron: cronCount }
})

// --- Send a test message (useful during development) ---

command("send-test-message", async ({ message }: { message: string }) => {
  const q = queue("inbox")
  await q.send(message)
  return { sent: true }
})

// --- HTTP surface ---
// The same Worker also serves HTTP. A Worker can expose an HTTP app (this
// default export), react to triggers (the handlers above), and answer commands
// all from one deployment.

const app = new Hono()

app.get("/health", c => c.json({ status: "ok" }))

// Enqueue a message over HTTP. The response returns as soon as the message is
// queued; the audit-log write is handed to `waitUntil`, so it runs in the
// background after the response is sent instead of blocking the caller.
app.post("/ingest", async c => {
  const { message } = await c.req.json<{ message: string }>()
  const q = queue("inbox")
  await q.send(message)

  waitUntil(
    kv("events").setJson(`audit:${Date.now()}`, {
      type: "http-ingest",
      message,
      at: new Date().toISOString(),
    }),
  )

  return c.json({ queued: true })
})

app.get("/stats", async c => {
  const ev = kv("events")
  let queueCount = 0
  let storageCount = 0
  let cronCount = 0
  for await (const _ of scanAll(ev, "queue:")) queueCount++
  for await (const _ of scanAll(ev, "storage:")) storageCount++
  for await (const _ of scanAll(ev, "cron:")) cronCount++
  return c.json({ queue: queueCount, storage: storageCount, cron: cronCount })
})

export default app
