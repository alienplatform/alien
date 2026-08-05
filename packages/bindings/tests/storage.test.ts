/**
 * Storage behavioral tests through the REAL napi addon against the local
 * file-tree provider (`crates/alien-bindings/src/providers/storage/local.rs`).
 * No mocks: every op does real filesystem I/O under a fresh temp directory.
 */

import { randomUUID } from "node:crypto"
import { afterAll, describe, expect, it } from "vitest"
import type { Storage } from "../src/index.js"
import { storage } from "../src/index.js"
import { cleanupTempDirs, localStorageBindingEnv } from "./helpers/local-binding-env.js"

let bunFixtureIndex = 0

function freshStorage(): Storage {
  const isBun = process.env.BUN_EXPECTED === "1"
  const name = isBun ? `bun-storage-${bunFixtureIndex++}` : `storage-${randomUUID()}`
  if (!isBun) localStorageBindingEnv(name)
  return storage(name)
}

describe("storage (local file-tree provider)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  it("round-trips a Buffer byte-exact, including zero bytes and arbitrary binary content", async () => {
    const s = freshStorage()
    const data = Buffer.from([0x00, 0x01, 0x02, 0xff, 0xfe, 0x00, 0x80, 0x7f, 0x00])

    const putResult = await s.put("bin/data.bin", data)
    const fetched = await s.get("bin/data.bin")

    expect(putResult.eTag).toEqual(expect.any(String))
    expect(putResult.version).toBeUndefined()
    expect(fetched.data.length).toBe(data.length)
    expect(fetched.data.equals(data)).toBe(true)
    expect(fetched.meta).toMatchObject({ location: "bin/data.bin", size: data.length })
    expect(Number.isNaN(Date.parse(fetched.meta.lastModified))).toBe(false)
    expect(fetched.attributes).toEqual({ metadata: {} })
  })

  it("rejects object attributes when the backend cannot persist them", async () => {
    const s = freshStorage()
    const data = Buffer.from("attribute-bearing object")

    await expect(
      s.put("attributes.txt", data, {
        attributes: {
          contentType: "text/plain; charset=utf-8",
          contentDisposition: 'attachment; filename="attributes.txt"',
          contentEncoding: "identity",
          contentLanguage: "en-US",
          cacheControl: "private, max-age=60",
          metadata: { source: "integration-test", checksum: "abc123" },
        },
      }),
    ).rejects.toMatchObject({
      code: "OPERATION_NOT_SUPPORTED",
      retryable: false,
    })
    expect(await s.list()).toEqual([])
  })

  it("lists, heads, copies, and deletes objects", async () => {
    const s = freshStorage()
    await s.put("a.txt", Buffer.from("a-content"))
    await s.put("b.txt", Buffer.from("b-content"))

    const listed = await s.list()
    expect(listed.map(o => o.location).sort()).toEqual(["a.txt", "b.txt"])

    const head = await s.head("a.txt")
    expect(head.meta.location).toBe("a.txt")
    expect(head.meta.size).toBe(Buffer.from("a-content").length)
    expect(Number.isNaN(Date.parse(head.meta.lastModified))).toBe(false)
    expect(head.attributes).toEqual({ metadata: {} })

    await s.copy("a.txt", "a-copy.txt")
    expect((await s.get("a-copy.txt")).data.toString("utf8")).toBe("a-content")
    // copy must not remove the source
    expect((await s.get("a.txt")).data.toString("utf8")).toBe("a-content")

    await s.delete("a.txt")
    const afterDelete = await s.list()
    expect(afterDelete.map(o => o.location).sort()).toEqual(["a-copy.txt", "b.txt"])
  })

  it("filters list() by prefix", async () => {
    const s = freshStorage()
    await s.put("dir/one.txt", Buffer.from("1"))
    await s.put("dir/two.txt", Buffer.from("2"))
    await s.put("other.txt", Buffer.from("3"))

    const listed = await s.list("dir")

    expect(listed.map(o => o.location).sort()).toEqual(["dir/one.txt", "dir/two.txt"])
  })

  it("signedUrl returns a {url, method, headers} presigned request shape", async () => {
    const s = freshStorage()
    await s.put("signed.txt", Buffer.from("hi"))

    const req = await s.signedUrl({ method: "GET", path: "signed.txt", expiresIn: 60 })

    expect(req.method).toBe("GET")
    expect(req.url.startsWith("local://")).toBe(true)
    expect(req.url).toContain("signed.txt")
    expect(req.headers).toEqual({})
  })

  it("signedUrl reflects the requested method (PUT / DELETE)", async () => {
    const s = freshStorage()

    const put = await s.signedUrl({ method: "PUT", path: "x.txt", expiresIn: 60 })
    expect(put.method).toBe("PUT")

    const del = await s.signedUrl({ method: "DELETE", path: "x.txt", expiresIn: 60 })
    expect(del.method).toBe("DELETE")
  })
})
