import { queue } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/queue-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const q = queue(bindingName)

    // 1. Send message
    await q.send({ test: true, ts: Date.now() })

    // 2. Receive message
    const messages = await q.receive(1)
    if (messages.length === 0) {
      return c.json({ success: false, error: "No messages received" }, 500)
    }

    // 3. Acknowledge message
    const msg = messages[0]
    await q.ack(msg.receiptHandle)

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "queue-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

// Send-only: the platform queue trigger is the only consumer of this queue, so
// delivery proves the trigger invoked the app's onQueueMessage handler (which
// records the message in KV for read-back via /events/list).
app.post("/queue-send/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const { marker } = (await c.req.json()) as { marker: string }
    if (!marker) {
      return c.json({ success: false, error: "Missing marker" }, 400)
    }
    const q = queue(bindingName)
    await q.send({ marker })
    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "queue-send")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
