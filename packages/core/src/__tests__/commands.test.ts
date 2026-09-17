import { describe, expect, it } from "vitest"
import { CreateCommandResponseSchema } from "../index.js"

describe("command schemas", () => {
  it("accepts create responses from managers that predate the created field", () => {
    const response = CreateCommandResponseSchema.parse({
      commandId: "cmd_123",
      state: "PENDING",
      inlineAllowedUpTo: 1024,
      next: "poll",
    })

    expect(response.created).toBeUndefined()
  })
})
