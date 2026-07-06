import { type Deployment, deploy } from "@alienplatform/testing"
import { afterAll, beforeAll, describe, expect, it } from "vitest"

describe("remote-worker-ts", () => {
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
  })

  it("should list available tools", async () => {
    const tools = await deployment.invokeCommand("list-tools", {})

    expect(Array.isArray(tools)).toBe(true)
    const names = tools.map((t: { name: string }) => t.name)
    expect(names).toContain("read-file")
    expect(names).toContain("write-file")
  })

  it("should write and read a file", async () => {
    await deployment.invokeCommand("execute-tool", {
      tool: "write-file",
      params: { path: "hello.txt", content: "Hello!" },
    })

    const result = await deployment.invokeCommand("execute-tool", {
      tool: "read-file",
      params: { path: "hello.txt" },
    })

    expect(result.content).toBe("Hello!")
  })

  it("should reject unknown tools", async () => {
    await expect(
      deployment.invokeCommand("execute-tool", {
        tool: "nonexistent",
        params: {},
      }),
    ).rejects.toThrow()
  })
})
