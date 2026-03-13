/**
 * Example using Vitest
 *
 * This shows the recommended pattern for writing tests with the testing framework.
 */

import { type Deployment, deploy } from "@aliendotdev/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("My Alien App", () => {
  let deployment: Deployment

  beforeAll(async () => {
    deployment = await deploy({
      app: "./fixtures/hello-world",
      platform: "local",
    })
  }, 180_000) // 3 min timeout for deployment

  afterAll(async () => {
    if (deployment) {
      await deployment.destroy()
    }
  })

  it("should respond with 200", async () => {
    const response = await fetch(`${deployment.url}/api/health`)
    expect(response.status).toBe(200)
  })

  it("should return correct data", async () => {
    const response = await fetch(`${deployment.url}/api/hello`)
    const data = await response.json()

    expect(data).toEqual({
      message: "Hello, World!",
    })
  })

  it("should handle POST requests", async () => {
    const response = await fetch(`${deployment.url}/api/echo`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message: "test" }),
    })

    const data = await response.json()
    expect(data.message).toBe("test")
  })
})
