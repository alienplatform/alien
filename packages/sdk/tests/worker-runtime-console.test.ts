import { describe, expect, it } from "vitest"
import { formatWorkerConsoleLine } from "../src/worker-runtime/console.js"

describe("Worker console formatting", () => {
  it("preserves ordinary messages", () => {
    expect(formatWorkerConsoleLine(["worker ready"])).toBe("worker ready")
  })

  it("keeps object and Error output on one line without truncating it", () => {
    const line = formatWorkerConsoleLine([
      "request failed:",
      {
        id: "task_1",
        error: new Error(`first\nsecond ${"x".repeat(5_000)}`),
      },
    ])

    expect(line).not.toContain("\n")
    expect(line.startsWith("request failed: { id: 'task_1', error: Error: first second ")).toBe(
      true,
    )
    expect(line).toContain("x".repeat(5_000))
  })

  it("never throws when inspection fails", () => {
    const value = {
      [Symbol.for("nodejs.util.inspect.custom")]() {
        throw new Error("inspect failed")
      },
    }

    expect(formatWorkerConsoleLine([value])).toBe(
      "[alien:console] Log arguments could not be formatted",
    )
  })
})
