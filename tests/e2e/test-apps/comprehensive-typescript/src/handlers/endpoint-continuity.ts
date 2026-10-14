import { randomUUID } from "node:crypto"
import { Hono } from "hono"

const app = new Hono()
const processId = randomUUID()
const startedAt = Date.now()
let activeStreams = 0

app.get("/endpoint-continuity", c => c.json({ processId, startedAt, activeStreams }))

// A single response stays open across routing updates. Consumers must keep the
// same connectionId and consecutive sequence numbers; reconnecting is failure.
app.get("/endpoint-continuity/stream", c => {
  const count = Number(c.req.query("count") ?? "1800")
  const intervalMs = Number(c.req.query("intervalMs") ?? "1000")
  if (
    !Number.isInteger(count) ||
    count < 1 ||
    count > 3600 ||
    !Number.isInteger(intervalMs) ||
    intervalMs < 10 ||
    intervalMs > 30000
  ) {
    return c.json({ error: "Expected count 1–3600 and intervalMs 10–30000" }, 400)
  }

  const connectionId = randomUUID()
  const encoder = new TextEncoder()
  let sequence = 0
  let closed = false
  let cancelDelay: (() => void) | undefined
  activeStreams++
  const finish = () => {
    if (closed) return
    closed = true
    activeStreams--
    cancelDelay?.()
  }
  const body = new ReadableStream<Uint8Array>({
    async pull(controller) {
      if (sequence > 0) {
        await new Promise<void>(resolve => {
          const timer = setTimeout(resolve, intervalMs)
          cancelDelay = () => {
            clearTimeout(timer)
            resolve()
          }
        })
        cancelDelay = undefined
      }
      if (closed) return
      const frame = { processId, connectionId, sequence, elapsedMs: Date.now() - startedAt }
      controller.enqueue(encoder.encode(`data: ${JSON.stringify(frame)}\n\n`))
      sequence++
      if (sequence === count) {
        finish()
        controller.close()
      }
    },
    cancel: finish,
  })
  return new Response(body, {
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-store" },
  })
})

export default app
