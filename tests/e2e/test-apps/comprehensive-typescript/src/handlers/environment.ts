import { Hono } from "hono"

const app = new Hono()

app.get("/env-var/:varName", c => {
  const varName = c.req.param("varName")
  const value = process.env[varName]
  if (!value) {
    return c.json(
      {
        success: false,
        name: varName,
        value: null,
        error: `Environment variable ${varName} not found`,
      },
      404,
    )
  }
  return c.json({ success: true, name: varName, value })
})

export default app
