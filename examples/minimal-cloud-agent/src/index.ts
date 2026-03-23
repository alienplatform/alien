import { command } from "@alienplatform/bindings"
/**
 * Minimal Cloud Agent - src/index.ts
 *
 * The simplest possible Alien agent: one command handler.
 * Export a Hono app for HTTP endpoints (health checks).
 * Register the "echo" command for remote invocation.
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

// Command: echo
// Receives a message and returns it with a timestamp.
command("echo", async ({ message }: { message: string }) => {
  return {
    message,
    timestamp: new Date().toISOString(),
  }
})

export default app
