/**
 * Vault behavioral tests through the REAL napi addon against the local
 * secrets.json provider (`crates/alien-bindings/src/providers/vault/local.rs`).
 * No mocks: every op reads/writes the real `<dir>/secrets.json` file.
 */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { AlienError, vault } from "../src/index.js"
import type { Vault } from "../src/index.js"
import { cleanupTempDirs, localVaultBindingEnv } from "./helpers/local-binding-env.js"

let bunFixtureIndex = 0

function freshVault(): Vault {
  const isBun = process.env.BUN_EXPECTED === "1"
  const name = isBun ? `bun-vault-${bunFixtureIndex++}` : `vault-${randomUUID()}`
  if (!isBun) localVaultBindingEnv(name, "secrets")
  return vault(name)
}

describe("vault (local secrets.json provider)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("put/get round-trips a string secret", async () => {
    const v = freshVault()
    await v.put("api-key", "sekrit")

    expect(await v.get("api-key")).toBe("sekrit")
  })

  it("put overwrites an existing secret", async () => {
    const v = freshVault()
    await v.put("key", "first")
    await v.put("key", "second")

    expect(await v.get("key")).toBe("second")
  })

  it("putJson/getJson round-trips a JSON secret", async () => {
    const v = freshVault()
    await v.putJson("config", { host: "db.internal", port: 5432 })

    expect(await v.getJson("config")).toEqual({ host: "db.internal", port: 5432 })
  })

  it("delete removes a secret; a later get rejects", async () => {
    const v = freshVault()
    await v.put("temp", "x")
    await v.delete("temp")

    const err = await v.get("temp").catch((e: unknown) => e)
    expect(err).toBeInstanceOf(AlienError)
  })

  it("list returns every stored secret name", async () => {
    const v = freshVault()
    expect(await v.list()).toEqual([])

    await v.put("api-key", "1")
    await v.put("db-url", "2")

    const names = (await v.list()).slice().sort()
    expect(names).toEqual(["api-key", "db-url"])
  })
})
