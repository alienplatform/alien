/**
 * Redis behavioral tests through the REAL napi addon against the inline-password
 * providers. The binding JSON goes into the environment and the Rust resolver produces
 * the connection. Cloud backends read their password from a cloud secret store and are
 * covered by the cloud proofs on a deployed stack.
 */

import { afterAll, describe, expect, it } from "vitest"
import { redis } from "../src/index.js"
import {
  cleanupTempDirs,
  externalRedisBindingEnv,
  localRedisBindingEnv,
} from "./helpers/local-binding-env.js"

const ENCODED_PASSWORD = "a%21b%2Ac%27d%28e%29f%40%2F"

describe("redis (inline-password providers)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("resolves a local binding to a plaintext connection with an encoded password", async () => {
    localRedisBindingEnv("redis-local")
    const connection = await redis("redis-local").connection()

    expect(connection.url).toBe(`redis://default:${ENCODED_PASSWORD}@cache.internal:6379/0`)
    expect(connection.tls).toBe(false)
    expect(connection.database).toBe(0)
    // The separate fields keep the raw password for clients that take them apart.
    expect(connection.password).toBe("a!b*c'd(e)f@/")
    expect(connection.username).toBe("default")
  })

  it("resolves an external binding to TLS on the system trust store by default", async () => {
    externalRedisBindingEnv("redis-external")
    const connection = await redis("redis-external").connection()

    expect(connection.url).toBe(`rediss://default:${ENCODED_PASSWORD}@cache.internal:6379/0`)
    // No provider CA and no name override: an empty options object, not `false`.
    expect(connection.tls).toEqual({})
  })

  it("honors an explicit plaintext opt-out on an external binding", async () => {
    externalRedisBindingEnv("redis-external-plain", false)
    const connection = await redis("redis-external-plain").connection()

    expect(connection.url.startsWith("redis://")).toBe(true)
    expect(connection.tls).toBe(false)
  })

  it("rejects a name that has no binding", async () => {
    await expect(redis("redis-not-configured").connection()).rejects.toThrow()
  })
})
