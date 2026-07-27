import { afterEach, describe, expect, test, vi } from "vitest"
import { logSystemError } from "../src/worker-runtime/system-log.js"

describe("worker runtime system logs", () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  test("marks SDK-owned messages for the worker runtime", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {})

    logSystemError("[alien:event-loop] Task error: id=task_123")

    expect(error).toHaveBeenCalledWith(
      "\u001eALIEN_SYSTEM\u001f[alien:event-loop] Task error: id=task_123",
    )
  })
})
