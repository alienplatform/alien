import { randomUUID } from "node:crypto"
import { AlienError, defineError } from "@alienplatform/core"
import { onQueueBatch, queue, storage } from "@alienplatform/sdk"
import { Hono } from "hono"
import { z } from "zod/v4"

const OperationFailed = defineError({
  code: "DEAD_LETTER_TEST_OPERATION_FAILED",
  context: z.object({ operation: z.string() }),
  message: ({ operation }) => `Dead-letter test operation '${operation}' failed`,
  retryable: false,
  internal: false,
  httpStatusCode: 500,
})

/** `poison` messages always fail; every other message succeeds. */
const taskSchema = z.object({ id: z.string().min(1), poison: z.boolean() })
type Task = z.infer<typeof taskSchema>
const sendRequest = z.object({
  good: z.number().int().min(0).max(1000),
  poison: z.number().int().min(0).max(1000),
  prefix: z.string().min(1).default("m"),
})

const app = new Hono()
app.onError(async (error, c) => {
  const failure = (await AlienError.from(error)).withContext(
    OperationFailed.create({ operation: `${c.req.method} ${c.req.path}` }),
  )
  console.error(JSON.stringify(failure.toOptions()))
  return c.json({ code: failure.code, message: failure.message }, 500)
})
app.get("/health", c => c.json({ status: "ok" }))

/** Sends `good` good messages and `poison` poison ones to `tasks`. */
app.post("/send", async c => {
  const request = sendRequest.parse(await c.req.json())
  const tasks: Task[] = [
    ...Array.from({ length: request.good }, (_, n) => ({
      id: `${request.prefix}-good-${n}`,
      poison: false,
    })),
    ...Array.from({ length: request.poison }, (_, n) => ({
      id: `${request.prefix}-poison-${n}`,
      poison: true,
    })),
  ]
  const tasksQueue = queue("tasks")
  const batches: Task[][] = []
  for (let start = 0; start < tasks.length; start += 10) {
    batches.push(tasks.slice(start, start + 10))
  }
  await Promise.all(batches.map(batch => tasksQueue.sendBatch(batch)))
  return c.json({ sent: tasks.length })
})

/** One record per delivery, so redelivery is visible: id and the attempt the cloud reported. */
app.get("/deliveries", async c => {
  const results = storage("results")
  const records = await results.list("deliveries/")
  return c.json(
    await Promise.all(
      records.map(async record => {
        const value = await results.get(record.location)
        return JSON.parse(new TextDecoder().decode(value.data)) as unknown
      }),
    ),
  )
})

/** Reads what arrived on `tasks-failed` with `receive` (the pull path) and acknowledges it. */
app.post("/failed/drain", async c => {
  const failed = queue("tasks-failed")
  const drained: { task: unknown; attempt: number }[] = []
  for (;;) {
    const messages = await failed.receive(10)
    if (messages.length === 0) break
    for (const message of messages) {
      drained.push({
        task: message.payloadType === "json" ? JSON.parse(message.payload) : message.payload,
        attempt: message.attempt,
      })
    }
    await failed.ackBatch(messages.map(message => message.receiptHandle))
  }
  return c.json(drained)
})

onQueueBatch<unknown>("*", async batch => {
  try {
    const parsed = batch.deliveries.map(delivery => ({
      delivery,
      task: taskSchema.parse(delivery.message.payload),
    }))
    const results = storage("results")
    await Promise.all(
      parsed.map(({ delivery, task }) =>
        results.put(
          `deliveries/${randomUUID()}.json`,
          new TextEncoder().encode(
            JSON.stringify({
              id: task.id,
              poison: task.poison,
              deliveryId: delivery.deliveryId,
              attempt: delivery.message.attemptCount,
            }),
          ),
        ),
      ),
    )
    // A `retry` is a failed delivery: the poison messages end in `tasks-failed`.
    return parsed.map(({ delivery, task }) => ({
      deliveryId: delivery.deliveryId,
      disposition: task.poison ? ("retry" as const) : ("success" as const),
    }))
  } catch (error) {
    throw (await AlienError.from(error)).withContext(
      OperationFailed.create({ operation: "process queue batch" }),
    )
  }
})

export default app
