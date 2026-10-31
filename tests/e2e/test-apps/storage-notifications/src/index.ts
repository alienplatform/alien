import { randomUUID } from "node:crypto"
import { AlienError, defineError } from "@alienplatform/core"
import { onQueueBatch, storage, storageEventFromMessage } from "@alienplatform/sdk"
import { Hono } from "hono"
import { z } from "zod/v4"

const OperationFailed = defineError({
  code: "NOTIFICATION_TEST_OPERATION_FAILED",
  context: z.object({ operation: z.string() }),
  message: ({ operation }) => `Notification test operation '${operation}' failed`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})
const writes = z
  .array(
    z.object({
      storage: z.enum(["objects", "second"]),
      key: z.string().min(1),
      content: z.string(),
    }),
  )
  .min(1)
  .max(100)
const app = new Hono()

app.onError(async (error, c) => {
  const failure = (await AlienError.from(error)).withContext(
    OperationFailed.create({ operation: `${c.req.method} ${c.req.path}` }),
  )
  console.error(JSON.stringify(failure.toOptions()))
  return c.json({ code: failure.code, message: failure.message }, 500)
})
app.get("/health", c => c.json({ status: "ok" }))
app.post("/objects", async c => {
  const objects = writes.parse(await c.req.json())
  await Promise.all(
    objects.map(object =>
      storage(object.storage).put(object.key, new TextEncoder().encode(object.content)),
    ),
  )
  return c.json({ written: objects.length })
})
app.get("/batches", async c => {
  const results = storage("results")
  const records = await results.list("batches/")
  return c.json(
    await Promise.all(
      records.map(async record => {
        const value = await results.get(record.location)
        return JSON.parse(new TextDecoder().decode(value.data)) as unknown
      }),
    ),
  )
})

onQueueBatch("*", async batch => {
  try {
    const events = await Promise.all(
      batch.deliveries.map(async delivery => ({
        messageId: delivery.message.id,
        deliveryId: delivery.deliveryId,
        event: await storageEventFromMessage(delivery.message),
      })),
    )
    // A fresh record for every invocation exposes redelivery; a key-indexed
    // projection would overwrite duplicates and could hide a broken receipt.
    await storage("results").put(
      `batches/${randomUUID()}.json`,
      new TextEncoder().encode(JSON.stringify({ source: batch.source, events })),
    )
    return batch.deliveries.map(delivery => ({
      deliveryId: delivery.deliveryId,
      disposition: "success" as const,
    }))
  } catch (error) {
    throw (await AlienError.from(error)).withContext(
      OperationFailed.create({ operation: "record queue batch" }),
    )
  }
})

export default app
