import { kv } from "@alienplatform/sdk"
import { Hono } from "hono"
import { sanitizeKvKeyPart, scanAll } from "../helpers.js"

const app = new Hono()

app.get("/events/list", async c => {
  try {
    const k = kv("alien-kv")
    const storageEvents: unknown[] = []
    const cronEvents: unknown[] = []
    const queueMessages: unknown[] = []

    for await (const entry of scanAll(k, "storage_event:")) {
      storageEvents.push(JSON.parse(new TextDecoder().decode(entry.value)))
    }
    for await (const entry of scanAll(k, "cron_event:")) {
      cronEvents.push(JSON.parse(new TextDecoder().decode(entry.value)))
    }
    for await (const entry of scanAll(k, "queue_message:")) {
      queueMessages.push(JSON.parse(new TextDecoder().decode(entry.value)))
    }

    return c.json({ storageEvents, cronEvents, queueMessages })
  } catch {
    return c.json({ storageEvents: [], cronEvents: [], queueMessages: [] })
  }
})

app.get("/events/storage/:key", async c => {
  const key = c.req.param("key")
  try {
    const k = kv("alien-kv")
    const sanitizedKey = sanitizeKvKeyPart(key)
    const data = await k.get(`storage_event:${sanitizedKey}`)
    if (!data) return c.json({ found: false })
    return c.json({ found: true, event: JSON.parse(new TextDecoder().decode(data)) })
  } catch {
    return c.json({ found: false })
  }
})

app.get("/events/cron/:schedule", async c => {
  const schedule = c.req.param("schedule")
  try {
    const k = kv("alien-kv")
    const sanitizedSchedule = sanitizeKvKeyPart(schedule)
    const data = await k.get(`cron_event:${sanitizedSchedule}`)
    if (!data) return c.json({ found: false })
    return c.json({ found: true, event: JSON.parse(new TextDecoder().decode(data)) })
  } catch {
    return c.json({ found: false })
  }
})

app.get("/events/queue/:messageId", async c => {
  const messageId = c.req.param("messageId")
  try {
    const k = kv("alien-kv")
    const sanitizedId = sanitizeKvKeyPart(messageId)
    const data = await k.get(`queue_message:${sanitizedId}`)
    if (!data) return c.json({ found: false })
    return c.json({ found: true, event: JSON.parse(new TextDecoder().decode(data)) })
  } catch {
    return c.json({ found: false })
  }
})

export default app
