/**
 * Kv behavioral tests through the REAL napi addon against the local
 * turso-backed provider (`crates/alien-bindings/src/providers/kv/local.rs`).
 * No mocks: every op does real (sqlite-backed) I/O under a fresh temp
 * directory.
 */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import { kv } from "../src/index.js"
import type { Kv } from "../src/index.js"
import { cleanupTempDirs, localKvBindingEnv } from "./helpers/local-binding-env.js"

function freshKv(): Kv {
  const name = `kv-${randomUUID()}`
  const { env } = localKvBindingEnv(name)
  return kv(name, { env })
}

describe("kv (local turso-backed provider)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("round-trips a raw Buffer via get/set", async () => {
    const k = freshKv()
    await k.set("greeting", "hello")

    const value = await k.get("greeting")

    expect(value).not.toBeNull()
    expect((value as Buffer).toString("utf8")).toBe("hello")
  })

  it("get returns null for an absent key", async () => {
    const k = freshKv()
    expect(await k.get("nope")).toBeNull()
  })

  it("getText reads back the UTF-8 string", async () => {
    const k = freshKv()
    await k.set("text-key", "plain text value")

    expect(await k.getText("text-key")).toBe("plain text value")
  })

  it("setJson/getJson round-trip a JSON value", async () => {
    const k = freshKv()
    await k.setJson("json-key", { hello: "world", n: 1, nested: { a: [1, 2, 3] } })

    expect(await k.getJson("json-key")).toEqual({ hello: "world", n: 1, nested: { a: [1, 2, 3] } })
  })

  it("ifNotExists: the second set returns false and the value is unchanged", async () => {
    const k = freshKv()

    expect(await k.set("once", "first", { ifNotExists: true })).toBe(true)
    expect(await k.set("once", "second", { ifNotExists: true })).toBe(false)

    expect(await k.getText("once")).toBe("first")
  })

  it("a plain (non-conditional) set always overwrites", async () => {
    const k = freshKv()
    await k.set("plain", "first")
    await k.set("plain", "second")

    expect(await k.getText("plain")).toBe("second")
  })

  it("ttl: a short-lived key expires and reads back absent", async () => {
    const k = freshKv()
    await k.set("short-lived", "value", { ttl: 1 })

    expect(await k.getText("short-lived")).toBe("value")
    expect(await k.exists("short-lived")).toBe(true)

    // Deliberately generous margin over the 1s ttl to avoid CI flakiness; the
    // only slow test in this suite.
    await new Promise(resolve => setTimeout(resolve, 2000))

    expect(await k.getText("short-lived")).toBeNull()
    expect(await k.exists("short-lived")).toBe(false)
  })

  it("exists reflects presence without a ttl", async () => {
    const k = freshKv()
    expect(await k.exists("absent")).toBe(false)

    await k.set("present", "x")
    expect(await k.exists("present")).toBe(true)
  })

  it("delete removes a key", async () => {
    const k = freshKv()
    await k.set("to-delete", "x")
    await k.delete("to-delete")

    expect(await k.exists("to-delete")).toBe(false)
    expect(await k.get("to-delete")).toBeNull()
  })

  it("scan paginates by prefix, limit, and cursor with exact page boundaries", async () => {
    const k = freshKv()
    for (const n of [1, 2, 3, 4, 5]) {
      await k.set(`item:0${n}`, `value-${n}`)
    }
    await k.set("other:1", "unrelated")

    const page1 = await k.scan("item:", 2)
    expect(page1.items.map(i => i.key)).toEqual(["item:01", "item:02"])
    expect(page1.items.map(i => i.value.toString("utf8"))).toEqual(["value-1", "value-2"])
    expect(page1.nextCursor).toBeDefined()

    const page2 = await k.scan("item:", 2, page1.nextCursor)
    expect(page2.items.map(i => i.key)).toEqual(["item:03", "item:04"])
    expect(page2.nextCursor).toBeDefined()

    const page3 = await k.scan("item:", 2, page2.nextCursor)
    expect(page3.items.map(i => i.key)).toEqual(["item:05"])
    expect(page3.nextCursor).toBeUndefined()

    // A single unpaginated scan sees every matching item and none of the unrelated prefix.
    const all = await k.scan("item:")
    expect(all.items.map(i => i.key)).toEqual([
      "item:01",
      "item:02",
      "item:03",
      "item:04",
      "item:05",
    ])
    expect(all.nextCursor).toBeUndefined()
  })
})
