import { Hono } from "hono"

const app = new Hono()

app.get("/health", c => {
  return c.json({ status: "ok", timestamp: new Date().toISOString() })
})

app.get("/hello", c => {
  return c.json({ message: "Hello from TypeScript!", timestamp: new Date().toISOString() })
})

export default app
