import { queue } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/queue-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const q = await queue(bindingName)

    // 1. Send message
    await q.send(bindingName, { test: true, ts: Date.now() })

    // 2. Receive message
    const messages = await q.receive(bindingName, 1)
    if (messages.length === 0) {
      return c.json({ success: false, error: "No messages received" }, 500)
    }

    // 3. Acknowledge message
    const msg = messages[0]
    await q.ack(bindingName, msg.receiptHandle)

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "queue-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
