import { describe, expect, it } from "vitest"
import { Key } from "../key.js"
import { Storage } from "../storage.js"

describe("Storage.encryptionKey", () => {
  it("stores a typed Key reference", () => {
    const key = new Key("customer-key").build()
    const storage = new Storage("customer-data").encryptionKey(key).build()

    expect(storage.config).toMatchObject({
      type: "storage",
      encryptionKey: { type: "key", id: "customer-key" },
    })
  })

  it("rejects non-Key resources", () => {
    const otherStorage = new Storage("not-a-key").build()
    expect(() => new Storage("customer-data").encryptionKey(otherStorage)).toThrow(
      "requires an alien.Key",
    )
  })
})
