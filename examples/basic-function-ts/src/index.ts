import { command } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

app.get("/health", c => {
  return c.json({
    status: "ok",
    timestamp: new Date().toISOString(),
  })
})

command("echo", async params => {
  return {
    ...params,
    timestamp: new Date().toISOString(),
  }
})

export default app
