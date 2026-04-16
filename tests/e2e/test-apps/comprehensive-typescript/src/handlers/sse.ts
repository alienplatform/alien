import { Hono } from "hono"

const app = new Hono()

app.get("/sse", c => {
  const encoder = new TextEncoder()
  const stream = new ReadableStream({
    start(controller) {
      for (let i = 0; i < 10; i++) {
        controller.enqueue(encoder.encode(`data: sse_message_${i}\n\n`))
      }
      controller.close()
    },
  })
  return new Response(stream, {
    headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" },
  })
})

export default app
