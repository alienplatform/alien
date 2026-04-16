import { storage, waitUntil } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/wait-until-test", async c => {
  const { storageBindingName, testData, delayMs } = await c.req.json()
  const testId = `test-${Date.now()}`

  waitUntil(
    (async () => {
      await new Promise(resolve => setTimeout(resolve, delayMs || 1000))
      const s = await storage(storageBindingName || "alien-storage")
      await s.put(`wait-until-${testId}.txt`, testData || "background-task-done")
    })(),
  )

  return c.json({ success: true, testId, message: "Background task scheduled" })
})

app.get("/wait-until-verify/:testId/:storageBindingName", async c => {
  const testId = c.req.param("testId")
  const storageBindingName = c.req.param("storageBindingName")
  try {
    const s = await storage(storageBindingName)
    const exists = await s.exists(`wait-until-${testId}.txt`)
    if (!exists) {
      return c.json({
        success: false,
        testId,
        backgroundTaskCompleted: false,
        message: "File not found yet",
      })
    }
    const result = await s.get(`wait-until-${testId}.txt`)
    const fileContent = new TextDecoder().decode(result.data)
    return c.json({
      success: true,
      testId,
      backgroundTaskCompleted: true,
      fileContent,
      message: "Background task completed",
    })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "wait-until-verify")
    return c.json({
      success: false,
      testId,
      backgroundTaskCompleted: false,
      message: alienError.message,
      code: alienError.code,
    })
  }
})

export default app
