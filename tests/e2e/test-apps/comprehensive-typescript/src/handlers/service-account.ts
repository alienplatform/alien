import { serviceAccount } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/service-account-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const sa = await serviceAccount(bindingName)

    // Get service account identity info
    const info = await sa.getInfo()

    return c.json({ success: true, bindingName, info })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "service-account-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
