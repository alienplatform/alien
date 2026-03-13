/**
 * Test application for Alien local platform integration tests.
 *
 * Simply export default a Hono app - the Alien bootstrap handles:
 * - Starting HTTP server on a random port
 * - Registering with the runtime
 * - Entering the event loop
 */

import { Hono } from "hono"

const app = new Hono()

// Health check / default route
app.get("/", c => {
  return c.json({ status: "ok", message: "Hello from test-app!" })
})

// Test route that returns request info
app.get("/*", c => {
  return c.json({
    status: "ok",
    method: "GET",
    path: c.req.path,
  })
})

// POST handler
app.post("/*", async c => {
  const body = await c.req.text()
  return c.json({
    status: "ok",
    method: "POST",
    path: c.req.path,
    body_length: body.length,
  })
})

export default app
