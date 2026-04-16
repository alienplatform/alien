import { Hono } from "hono"

const app = new Hono()

app.post("/inspect", async c => {
  const body = await c.req.json()
  return c.json({ success: true, requestBody: body })
})

export default app
