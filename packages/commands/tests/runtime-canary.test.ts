/**
 * Runtime-identity canary: makes the "runs under real Bun" claim for
 * `test:bun` self-enforcing instead of aspirational.
 *
 * This exact file executes under both `vitest run` (Node, via `test`) and
 * `bun test` (Bun's native test runner, via `test:bun`). It records which
 * runtime ran it and — since `test:bun` sets `BUN_EXPECTED=1` — asserts that
 * a run made under that env var actually observes the `Bun` global. If
 * `test:bun` ever regresses to executing under Node, this test fails instead
 * of the suite quietly passing for the wrong reason.
 */

import { describe, expect, it } from "vitest"

describe("runtime canary", () => {
  it("proves which runtime executed this file", () => {
    const isBun = typeof (globalThis as { Bun?: unknown }).Bun !== "undefined"
    console.log(`[runtime-canary] executing under ${isBun ? "Bun" : "Node"}`)

    if (process.env.BUN_EXPECTED === "1") {
      expect(isBun).toBe(true)
    } else {
      expect(isBun).toBe(false)
    }
  })
})
