/**
 * Tests for the Minimal Cloud Agent
 *
 * Uses @aliendotdev/testing with the "dev" deployment method for pure local testing.
 * No Agent Manager, no API calls, no cloud dependencies.
 */

import { type Deployment, deploy } from "@aliendotdev/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("Minimal Cloud Agent", () => {
  let deployment: Deployment

  beforeAll(async () => {
    // Deploy using alien dev directly - no API or Agent Manager needed
    deployment = await deploy({
      app: ".",
      platform: "local",
    })
  }, 300_000) // 5 minute timeout for build + deploy

  afterAll(async () => {
    await deployment?.destroy()
  })

  describe("HTTP Endpoints", () => {
    it("should respond to health check", async () => {
      const response = await fetch(`${deployment.url}/health`)

      expect(response.ok).toBe(true)

      const data = await response.json()
      expect(data.status).toBe("ok")
      expect(data.timestamp).toBeDefined()
    })
  })

  describe("ARC Commands", () => {
    it("should execute echo command", async () => {
      const message = "hello from test"

      const result = await deployment.invokeCommand("echo", { message })

      expect(result.message).toBe(message)
      expect(result.timestamp).toBeDefined()
    })

    it("should handle complex messages in echo command", async () => {
      const message = "complex message with special chars: !@#$%^&*()"

      const result = await deployment.invokeCommand("echo", { message })

      expect(result.message).toBe(message)
    })

    it("should return timestamp in ISO format", async () => {
      const result = await deployment.invokeCommand("echo", { message: "test" })

      // Verify timestamp is a valid ISO date string
      const timestamp = new Date(result.timestamp)
      expect(timestamp.getTime()).toBeGreaterThan(0)
    })
  })
})
