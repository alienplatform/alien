import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("basic-function-ts", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({ app: ".", platform: "local" })
  }, 300_000)

  afterAll(async () => {
    await deployment?.destroy()
  })

  it("should respond to health check", async () => {
    const response = await fetch(`${deployment.url}/health`)
    expect(response.ok).toBe(true)

    const data = await response.json()
    expect(data.status).toBe("ok")
    expect(data.timestamp).toBeDefined()
  })

  it("should execute echo command", async () => {
    const result = await deployment.invokeCommand("echo", { message: "hello" })
    expect(result.message).toBe("hello")
    expect(result.timestamp).toBeDefined()
  })

  it("should echo back all params", async () => {
    const result = await deployment.invokeCommand("echo", { a: 1, b: "two" })
    expect(result.a).toBe(1)
    expect(result.b).toBe("two")
  })
})
