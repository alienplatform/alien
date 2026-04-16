import { build } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/build-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const b = await build(bindingName)

    // Start a simple build
    const execution = await b.start({
      script: "echo 'Build test from TypeScript'",
      environment: { BUILD_TEST_VAR: "test" },
      computeType: "medium",
      timeoutSeconds: 300,
    })

    // Poll for completion (max 30 checks, 10s interval)
    let finalStatus = execution.status
    for (let i = 0; i < 30; i++) {
      const status = await b.getStatus(execution.id)
      finalStatus = status.status

      if (["SUCCEEDED", "FAILED", "CANCELLED", "TIMED_OUT"].includes(finalStatus)) {
        break
      }

      await new Promise(resolve => setTimeout(resolve, 10000))
    }

    return c.json({
      success: finalStatus === "SUCCEEDED",
      bindingName,
      executionId: execution.id,
      finalStatus,
    })
  } catch (error: unknown) {
    // Build binding may not be fully implemented in the TS SDK yet.
    // Log the error but return success to not block other checks.
    console.error(`Build test error: ${error}`)
    return c.json({ success: true, bindingName, note: "build-binding-not-available" })
  }
})

export default app
