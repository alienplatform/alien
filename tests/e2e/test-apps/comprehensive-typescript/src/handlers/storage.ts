import { storage } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/storage-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const s = await storage(bindingName)
    const testKey = `test-${Date.now()}.txt`
    const content = "test content from e2e"

    // 1. Put
    await s.put(testKey, content)

    // 2. Get and verify
    const retrieved = await s.get(testKey)
    const retrievedContent = new TextDecoder().decode(retrieved.data)
    if (retrievedContent !== content) {
      return c.json({ success: false, error: "Data verification failed" }, 500)
    }

    // 3. Delete
    await s.delete(testKey)

    // 4. Head after delete — should throw NotFound
    try {
      await s.head(testKey)
      return c.json({ success: false, error: "Head after delete should have thrown" }, 500)
    } catch {
      // Expected: NotFound
    }

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "storage-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
