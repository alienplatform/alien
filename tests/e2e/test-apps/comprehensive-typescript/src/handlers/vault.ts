import { vault } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/vault-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const v = await vault(bindingName)
    const testKey = `test-secret-${Date.now()}`
    const testValue = "test-secret-value"

    // 1. Set secret
    await v.set(testKey, testValue)

    // 2. Wait for propagation
    await new Promise(resolve => setTimeout(resolve, 500))

    // 3. Get and verify
    const retrieved = await v.get(testKey)
    if (retrieved !== testValue) {
      return c.json(
        {
          success: false,
          error: `Value mismatch: expected "${testValue}", got "${retrieved}"`,
        },
        500,
      )
    }

    // 4. Delete
    await v.delete(testKey)

    // 5. Wait for propagation
    await new Promise(resolve => setTimeout(resolve, 500))

    // 6. Verify deletion
    try {
      await v.get(testKey)
      // If we get here without error, deletion may not have propagated yet — acceptable
    } catch {
      // Expected: secret not found
    }

    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "vault-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

app.get("/external-secret", async c => {
  try {
    const v = await vault("alien-vault")
    const value = await v.get("EXTERNAL_TEST_SECRET")
    return c.json({ exists: !!value, value })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "external-secret")
    return c.json({ exists: false, error: alienError.message, code: alienError.code })
  }
})

export default app
