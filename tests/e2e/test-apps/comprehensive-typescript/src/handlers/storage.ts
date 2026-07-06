import { storage } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/storage-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const s = storage(bindingName)
    const testKey = `test-${Date.now()}.txt`
    const content = "test content from e2e"

    // 1. Put
    await s.put(testKey, new TextEncoder().encode(content))

    // 2. Get and verify
    const retrieved = await s.get(testKey)
    const retrievedContent = new TextDecoder().decode(retrieved)
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

// Write-only storage operation. Unlike /storage-test, the object is not deleted
// afterwards, so the platform storage trigger observes exactly one `created`
// event for the key and the test can read back the record the onStorageEvent
// handler wrote via /events/storage/:key.
app.post("/storage-write/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const { key, content } = (await c.req.json()) as { key: string; content: string }
    if (!key || content === undefined) {
      return c.json({ success: false, error: "Missing key or content" }, 400)
    }
    const s = storage(bindingName)
    await s.put(key, new TextEncoder().encode(content))
    return c.json({ success: true, bindingName, key })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "storage-write")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
