import { kv } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/kv-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const k = await kv(bindingName)
    const prefix = `test-key-${Date.now()}`
    const key1 = `${prefix}-1`
    const key2 = `${prefix}-2`
    const testValue = { message: "kv-test", ts: Date.now() }

    // 1. Put
    await k.set(key1, testValue)

    // 2. Put with ifNotExists — second set should return false
    const firstSet = await k.set(key2, testValue, { ifNotExists: true })
    const secondSet = await k.set(key2, { message: "duplicate" }, { ifNotExists: true })
    if (secondSet !== false) {
      return c.json(
        { success: false, error: "ifNotExists: duplicate set should return false" },
        500,
      )
    }

    // 3. Get and verify
    const retrieved = await k.get(key1)
    if (!retrieved) {
      return c.json({ success: false, error: "Get returned undefined for existing key" }, 500)
    }
    const value = JSON.parse(new TextDecoder().decode(retrieved))
    if (value.message !== testValue.message) {
      return c.json({ success: false, error: "Value mismatch" }, 500)
    }

    // 4. Exists
    const exists = await k.exists(key1)
    if (!exists) {
      return c.json({ success: false, error: "Exists returned false for existing key" }, 500)
    }

    // 5. Scan prefix
    let scanCount = 0
    for await (const _ of k.scan(prefix)) {
      scanCount++
    }
    if (scanCount < 2) {
      return c.json({ success: false, error: `Scan found ${scanCount} items, expected >= 2` }, 500)
    }

    // 6. Delete and verify
    await k.delete(key1)
    const existsAfterDelete = await k.exists(key1)
    if (existsAfterDelete) {
      return c.json({ success: false, error: "Exists returned true after delete" }, 500)
    }
    const getAfterDelete = await k.get(key1)
    if (getAfterDelete !== undefined) {
      return c.json({ success: false, error: "Get returned value after delete" }, 500)
    }

    // Cleanup
    await k.delete(key2)

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "kv-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
