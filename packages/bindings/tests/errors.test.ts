/**
 * Behavioral error-contract tests through the REAL napi addon (no mocks): a
 * missing binding, malformed binding JSON, and a recognized-but-unsupported
 * provider tag each surface as the documented typed error. Unit-level
 * `unwrapNapiError` coverage (fake envelopes) lives in
 * `src/__tests__/errors.test.ts`; this file proves the addon actually
 * produces those envelopes.
 */

import { randomUUID } from "node:crypto"
import { afterEach, describe, expect, it } from "vitest"
import { AlienError, BindingNotConfiguredError, kv, storage } from "../src/index.js"
import {
  LOCAL_DEPLOYMENT_ENV,
  bindingEnvVarName,
  cleanupTempDirs,
  installBindingEnv,
} from "./helpers/local-binding-env.js"

afterEach(cleanupTempDirs)
const isBun = process.env.BUN_EXPECTED === "1"

describe("bindingEnvVarName", () => {
  it("derives storage('my-files') -> ALIEN_MY_FILES_BINDING", () => {
    expect(bindingEnvVarName("my-files")).toBe("ALIEN_MY_FILES_BINDING")
  })
})

describe("missing binding", () => {
  it("throws BindingNotConfiguredError with {binding, envVar, code} on the first operation", async () => {
    const name = `missing-${randomUUID()}`
    installBindingEnv(LOCAL_DEPLOYMENT_ENV)
    const s = storage(name)

    const err = await s.head("whatever").catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alienErr = err as AlienError
    expect(alienErr.code).toBe("BINDING_NOT_CONFIGURED")
    expect(alienErr.code).toBe(BindingNotConfiguredError.metadata.code)
    expect(alienErr.context).toEqual({ binding: name, envVar: bindingEnvVarName(name) })
  })
})

describe("malformed binding JSON", () => {
  it("throws BINDING_CONFIG_INVALID naming the env var", async () => {
    const name = isBun ? "bun-bad-json" : `bad-json-${randomUUID()}`
    if (!isBun) {
      installBindingEnv({ ...LOCAL_DEPLOYMENT_ENV, [bindingEnvVarName(name)]: "not-json" })
    }
    const s = storage(name)

    const err = await s.head("whatever").catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alienErr = err as AlienError
    expect(alienErr.code).toBe("BINDING_CONFIG_INVALID")
    expect(alienErr.message).toContain(bindingEnvVarName(name))
  })
})

describe("unsupported provider tag", () => {
  it("throws UNSUPPORTED_BINDING_PROVIDER for a recognized-but-unimplemented kv provider", async () => {
    const name = isBun ? "bun-redis" : `redis-${randomUUID()}`
    if (!isBun) {
      installBindingEnv({
        ...LOCAL_DEPLOYMENT_ENV,
        [bindingEnvVarName(name)]: JSON.stringify({
          service: "redis",
          connectionUrl: "redis://localhost:6379",
        }),
      })
    }
    const k = kv(name)

    const err = await k.exists("whatever").catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alienErr = err as AlienError
    expect(alienErr.code).toBe("UNSUPPORTED_BINDING_PROVIDER")
    expect(alienErr.message).toContain(bindingEnvVarName(name))
  })
})
