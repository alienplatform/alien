import { describe, expect, it, vi } from "vitest"
import { z } from "zod"
import { command, runCommand } from "../src/worker-runtime/registry.js"

const context = { commandId: "command-1", attempt: 1 }

describe("Worker command validation", () => {
  it("passes schema-less input as unknown", async () => {
    command("schema-less-test", input => ({ input }))
    await expect(runCommand("schema-less-test", "hello", context)).resolves.toEqual({
      input: "hello",
    })
  })

  it("accepts a Zod Standard Schema and infers its output", async () => {
    command("zod-test", z.object({ count: z.number() }), input => input.count * 2)
    await expect(runCommand("zod-test", { count: 4 }, context)).resolves.toBe(8)
  })

  it("awaits Standard Schema validation and skips the handler on failure", async () => {
    const handler = vi.fn((input: { name: string }) => input.name)
    const schema = {
      "~standard": {
        version: 1 as const,
        vendor: "test",
        types: undefined as unknown as { input: unknown; output: { name: string } },
        validate: async (value: unknown) =>
          typeof value === "object" &&
          value !== null &&
          "name" in value &&
          typeof value.name === "string"
            ? { value: { name: value.name } }
            : { issues: [{ message: "name must be a string" }] },
      },
    }
    command("validated-test", schema, handler)

    await expect(runCommand("validated-test", {}, context)).rejects.toThrow(
      "Command input failed validation: name must be a string",
    )
    expect(handler).not.toHaveBeenCalled()
    await expect(runCommand("validated-test", { name: "Ada" }, context)).resolves.toBe("Ada")
  })
})
