import { command, kv, onCronEvent, onQueueMessage, onStorageEvent, queue } from "@alienplatform/sdk"

// --- Event Handlers ---

// Process messages from the inbox queue
onQueueMessage("*", async message => {
  const ev = await kv("events")
  const payload =
    typeof message.payload === "string"
      ? message.payload
      : new TextDecoder().decode(message.payload as Uint8Array)

  await ev.set(`queue:${message.id}`, {
    type: "queue",
    source: message.source,
    payload,
    processedAt: new Date().toISOString(),
  })
})

// React to new files in storage
onStorageEvent("*", async event => {
  const ev = await kv("events")
  const key = event.objectKey.replace(/\//g, "_")

  await ev.set(`storage:${key}`, {
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
  const ev = await kv("events")

  await ev.set(`cron:${Date.now()}`, {
    type: "cron",
    schedule: event.scheduleName,
    scheduledTime: event.timestamp,
    processedAt: new Date().toISOString(),
  })
})

// --- Commands for querying processed events ---

command("get-events", async ({ type, limit }: { type?: string; limit?: number }) => {
  const ev = await kv("events")
  const prefix = type ? `${type}:` : ""
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

command("get-stats", async () => {
  const ev = await kv("events")
  let queueCount = 0
  let storageCount = 0
  let cronCount = 0

  for await (const _ of ev.scan("queue:")) queueCount++
  for await (const _ of ev.scan("storage:")) storageCount++
  for await (const _ of ev.scan("cron:")) cronCount++

  return { queue: queueCount, storage: storageCount, cron: cronCount }
})

// --- Send a test message (useful during development) ---

command("send-test-message", async ({ message }: { message: string }) => {
  const q = await queue("inbox")
  await q.send("test", message)
  return { sent: true }
})
