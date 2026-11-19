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

/**
 * One `receive` that gives up after `ms`. On Google Cloud a pull from an empty subscription can
 * wait longer than this Worker's request timeout, so an empty queue would otherwise fail the
 * whole request. A pull that is still waiting when we give up is left to expire. `timedOut` is
 * true when the receive gave up, which is not the same as the queue being empty: the callers
 * report it so a check that needs "empty" can ask again.
 */
async function receiveWithin(
  target: ReturnType<typeof queue>,
  max: number,
  ms = 8000,
): Promise<{
  messages: Awaited<ReturnType<ReturnType<typeof queue>["receive"]>>
  timedOut: boolean
}> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const timeout = new Promise<null>(resolve => {
    timer = setTimeout(() => resolve(null), ms)
  })
  try {
    const received = await Promise.race([target.receive(max), timeout])
    return { messages: received ?? [], timedOut: received === null }
  } finally {
    clearTimeout(timer)
  }
}

const app = new Hono()
app.onError(async (error, c) => {
  const failure = (await AlienError.from(error)).withContext(
    OperationFailed.create({ operation: `${c.req.method} ${c.req.path}` }),
  )
  console.error(JSON.stringify(failure.toOptions()))
  // The test harness shows the whole chain so a failing cloud call is diagnosable.
  return c.json({ code: failure.code, message: failure.message, chain: failure.toOptions() }, 500)
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
  let timedOut = false
  for (;;) {
    const received = await receiveWithin(failed, 10)
    const messages = received.messages
    if (messages.length === 0) {
      timedOut = received.timedOut
      break
    }
    for (const message of messages) {
      drained.push({
        task: message.payloadType === "json" ? JSON.parse(message.payload) : message.payload,
        attempt: message.attempt,
      })
    }
    await failed.ackBatch(messages.map(message => message.receiptHandle))
  }
  return c.json({ messages: drained, timedOut })
})

/** The pull path: `pulled` has no trigger. */
app.post("/pull/send", async c => {
  const request = sendRequest.parse(await c.req.json())
  const messages: Task[] = [
    ...Array.from({ length: request.good }, (_, n) => ({
      id: `${request.prefix}-good-${n}`,
      poison: false,
    })),
    ...Array.from({ length: request.poison }, (_, n) => ({
      id: `${request.prefix}-poison-${n}`,
      poison: true,
    })),
  ]
  const pulled = queue("pulled")
  for (let start = 0; start < messages.length; start += 10) {
    await pulled.sendBatch(messages.slice(start, start + 10))
  }
  return c.json({ sent: messages.length })
})

/** Receives from `pulled` once: acknowledges good messages, nacks (fails) the poison ones. */
app.post("/pull/consume", async c => {
  const pulled = queue("pulled")
  const results = storage("results")
  const messages = await pulled.receive(10)
  const seen: { id: string; poison: boolean; attempt: number }[] = []
  for (const message of messages) {
    const task = taskSchema.parse(JSON.parse(message.payload))
    seen.push({ id: task.id, poison: task.poison, attempt: message.attempt })
    await results.put(
      `pull-deliveries/${randomUUID()}.json`,
      new TextEncoder().encode(
        JSON.stringify({ id: task.id, poison: task.poison, attempt: message.attempt }),
      ),
    )
    if (!task.poison) await pulled.ack(message.receiptHandle)
    // `?nack=0`: leave a poison message locked and let the lock expire (the failure is the same).
    else if (c.req.query("nack") !== "0") await pulled.nack(message.receiptHandle)
  }
  return c.json(seen)
})

app.get("/pull/deliveries", async c => {
  const results = storage("results")
  const records = await results.list("pull-deliveries/")
  return c.json(
    await Promise.all(
      records.map(async record => {
        const value = await results.get(record.location)
        return JSON.parse(new TextDecoder().decode(value.data)) as unknown
      }),
    ),
  )
})

app.post("/pull/failed/drain", async c => {
  const failed = queue("pulled-failed")
  const drained: { task: unknown; attempt: number }[] = []
  let timedOut = false
  for (;;) {
    const received = await receiveWithin(failed, 10)
    const messages = received.messages
    if (messages.length === 0) {
      timedOut = received.timedOut
      break
    }
    for (const message of messages) {
      drained.push({
        task: message.payloadType === "json" ? JSON.parse(message.payload) : message.payload,
        attempt: message.attempt,
      })
    }
    await failed.ackBatch(messages.map(message => message.receiptHandle))
  }
  return c.json({ messages: drained, timedOut })
})

/** Reads `pulled` and gives every message back; shows what is left. */
app.post("/pull/peek", async c => {
  const pulled = queue("pulled")
  const messages = await pulled.receive(10)
  if (c.req.query("nack") !== "0") {
    await Promise.all(messages.map(message => pulled.nack(message.receiptHandle)))
  }
  return c.json({ visible: messages.length })
})

/** Reads `tasks` with `receive` (the pull path) and gives every message back; shows what is left. */
app.post("/tasks/peek", async c => {
  const tasks = queue("tasks")
  const { messages, timedOut } = await receiveWithin(tasks, 10)
  await Promise.all(messages.map(message => tasks.nack(message.receiptHandle)))
  return c.json({ visible: messages.length, timedOut })
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
