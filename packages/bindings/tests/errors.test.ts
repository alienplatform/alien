/**
 * Behavioral error-contract tests through the REAL napi addon (no mocks): a
 * missing binding, malformed binding JSON, and a recognized-but-unsupported
 * provider tag each surface as the documented typed error. Unit-level
 * `unwrapNapiError` coverage (fake envelopes) lives in
 * `src/__tests__/errors.test.ts`; this file proves the addon actually
 * produces those envelopes.
 */

import { randomUUID } from "node:crypto"
import { describe, expect, it } from "vitest"
import { AlienError, BindingNotConfiguredError, kv, storage } from "../src/index.js"
import { LOCAL_DEPLOYMENT_ENV, bindingEnvVarName } from "./helpers/local-binding-env.js"

describe("bindingEnvVarName", () => {
  it("derives storage('my-files') -> ALIEN_MY_FILES_BINDING", () => {
    expect(bindingEnvVarName("my-files")).toBe("ALIEN_MY_FILES_BINDING")
  })
})

describe("missing binding", () => {
  it("throws BindingNotConfiguredError with {binding, envVar, code} on the first operation", async () => {
    const name = `missing-${randomUUID()}`
    const s = storage(name, { env: LOCAL_DEPLOYMENT_ENV })

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
    const name = `bad-json-${randomUUID()}`
    const s = storage(name, {
      env: { ...LOCAL_DEPLOYMENT_ENV, [bindingEnvVarName(name)]: "not-json" },
    })

    const err = await s.head("whatever").catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alienErr = err as AlienError
    expect(alienErr.code).toBe("BINDING_CONFIG_INVALID")
    expect(alienErr.message).toContain(bindingEnvVarName(name))
  })
})

describe("unsupported provider tag", () => {
  it("throws UNSUPPORTED_BINDING_PROVIDER for a recognized-but-unimplemented kv provider", async () => {
    const name = `redis-${randomUUID()}`
    const k = kv(name, {
      env: {
        ...LOCAL_DEPLOYMENT_ENV,
        [bindingEnvVarName(name)]: JSON.stringify({
          service: "redis",
          connectionUrl: "redis://localhost:6379",
        }),
      },
    })

    const err = await k.exists("whatever").catch((e: unknown) => e)

    expect(err).toBeInstanceOf(AlienError)
    const alienErr = err as AlienError
    expect(alienErr.code).toBe("UNSUPPORTED_BINDING_PROVIDER")
    expect(alienErr.message).toContain(bindingEnvVarName(name))
  })
})
