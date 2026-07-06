/**
 * Proves `options.env` is a complete, isolated source of binding
 * configuration: the process environment carries no `ALIEN_*` state, and a
 * factory resolves entirely from the per-call override map. Also proves the
 * `undefined`-filter (`factories.ts::filterEnv`) against the REAL addon: an
 * `undefined` value surviving into the napi call would fail to convert into
 * the addon's `Record<string, string>` argument, so a passing round-trip here
 * is a stronger proof than the mocked-addon coverage in
 * `src/__tests__/factories.test.ts`.
 */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { kv } from "../src/index.js"
import { cleanupTempDirs, localKvBindingEnv } from "./helpers/local-binding-env.js"

describe("env override isolation", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("has no ALIEN_*_BINDING variables in process.env (test isolation precondition)", () => {
    const stray = Object.keys(process.env).filter(
      key => key.startsWith("ALIEN_") && key.endsWith("_BINDING"),
    )
    expect(stray).toEqual([])
  })

  it("resolves a binding purely from options.env, with zero ALIEN_* in process.env", async () => {
    const name = `override-${randomUUID()}`
    const { env } = localKvBindingEnv(name)

    // Sanity: this binding's env var genuinely isn't in the real process env.
    expect(
      process.env[Object.keys(env).find(k => k.endsWith("_BINDING")) as string],
    ).toBeUndefined()

    const k = kv(name, { env })
    await k.set("k", "v")

    expect(await k.getText("k")).toBe("v")
  })

  it("drops undefined env values before they cross into the addon", async () => {
    const name = `override-undef-${randomUUID()}`
    const { env } = localKvBindingEnv(name)

    // If `undefined` reached the napi call unfiltered, the addon's
    // `Record<string, string>` conversion would throw; a clean round-trip
    // proves `filterEnv` stripped it first.
    const k = kv(name, {
      env: {
        ...env,
        SOME_UNRELATED_UNDEFINED_FLAG: undefined,
      },
    })

    await k.set("k", "v")
    expect(await k.getText("k")).toBe("v")
  })
})
