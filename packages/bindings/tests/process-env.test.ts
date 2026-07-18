/** Proves the public factories resolve binding configuration from process env. */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { kv } from "../src/index.js"
import { cleanupTempDirs, localKvBindingEnv } from "./helpers/local-binding-env.js"

describe("process environment", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("resolves a binding from process.env", async () => {
    const isBun = process.env.BUN_EXPECTED === "1"
    const name = isBun ? "bun-process-env" : `process-env-${randomUUID()}`
    if (!isBun) localKvBindingEnv(name)
    const k = kv(name)
    await k.set("k", "v")
    expect(await k.getText("k")).toBe("v")
  })
})
