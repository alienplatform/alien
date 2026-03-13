import { command } from "@alienplatform/bindings"
/**
 * Minimal Cloud Agent - src/index.ts
 *
 * The simplest possible Alien agent: one ARC command handler.
 * Export a Hono app for HTTP endpoints (health checks).
 * Register the "echo" command for ARC invocation.
 */
import { Hono } from "hono"

const app = new Hono()

// Health endpoint - returns agent status
app.get("/health", c => {
  return c.json({
    status: "ok",
    timestamp: new Date().toISOString(),
  })
})

// ARC command: echo
// Receives a message and returns it with a timestamp.
// Invoke via: curl -X POST http://localhost:8080/arc/echo -H "Content-Type: application/json" -d '{"message": "hello"}'
command("echo", async ({ message }: { message: string }) => {
  return {
    message,
    timestamp: new Date().toISOString(),
  }
})

export default app
