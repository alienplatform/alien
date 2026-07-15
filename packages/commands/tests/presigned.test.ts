/**
 * Unit tests for the shared presigned-transfer mechanics (src/presigned.ts):
 * expiration check, http/local backend dispatch, the `..` traversal guard,
 * and the explicit `allowLocal` policy switch. Runs identically under Node
 * (`vitest run`) and Bun (`bun test`).
 */

import { mkdtemp, readFile, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { AlienError } from "@alienplatform/core"
import { describe, expect, it } from "vitest"
import { downloadPresigned, redactUrlForError, uploadPresigned } from "../src/presigned.js"
import type { PresignedRequest } from "../src/protocol.js"

const FUTURE = new Date(Date.now() + 60_000).toISOString()
const PAST = new Date(Date.now() - 60_000).toISOString()

function httpRequest(url: string, expiration = FUTURE): PresignedRequest {
  return {
    backend: { type: "http", url, method: "GET", headers: { "x-check": "1" } },
    expiration,
    operation: "get",
    path: "bodies/abc",
  }
}

function localRequest(filePath: string, expiration = FUTURE): PresignedRequest {
  return {
    backend: { type: "local", filePath, operation: "get" },
    expiration,
    operation: "get",
    path: "bodies/abc",
  }
}

async function expectStorageError(promise: Promise<unknown>, reason: string): Promise<void> {
  const error = await promise.then(
    () => {
      throw new Error("expected a rejection")
    },
    e => e,
  )
  expect(error).toBeInstanceOf(AlienError)
  expect((error as AlienError).code).toBe("STORAGE_OPERATION_FAILED")
  expect((error as AlienError).context).toMatchObject({
    reason: expect.stringContaining(reason),
  })
}

describe("downloadPresigned", () => {
  it("redacts credentials from diagnostic URLs", () => {
    const secret = "do-not-log-this-token"
    const sanitized = redactUrlForError(
      `https://user:${secret}@storage.test/object?X-Amz-Signature=${secret}#fragment`,
    )

    expect(sanitized).toBe("https://storage.test/object")
    expect(sanitized).not.toContain(secret)
    expect(redactUrlForError(`/response?response_token=${secret}`)).toBe("/response")
    expect(redactUrlForError(`not a URL containing ${secret}`)).toBe("<invalid-url>")
  })

  it("rejects an expired presigned request before touching the backend", async () => {
    let fetched = false
    const fetchImpl = (async () => {
      fetched = true
      return new Response("nope")
    }) as unknown as typeof fetch
    await expectStorageError(
      downloadPresigned(httpRequest("http://storage.test/x", PAST), {
        fetchImpl,
        allowLocal: true,
      }),
      "expired",
    )
    expect(fetched).toBe(false)
  })

  it("downloads bytes over http with the presigned method and headers", async () => {
    const seen: { url?: string; method?: string; headers?: unknown } = {}
    const fetchImpl = (async (url: unknown, init?: RequestInit) => {
      seen.url = String(url)
      seen.method = init?.method
      seen.headers = init?.headers
      return new Response(new Uint8Array([1, 2, 3]))
    }) as unknown as typeof fetch

    const bytes = await downloadPresigned(httpRequest("http://storage.test/x"), {
      fetchImpl,
      allowLocal: false,
    })
    expect(Array.from(bytes)).toEqual([1, 2, 3])
    expect(seen.url).toBe("http://storage.test/x")
    expect(seen.method).toBe("GET")
    expect(seen.headers).toEqual({ "x-check": "1" })
  })

  it("fails loudly on a non-2xx http response", async () => {
    const fetchImpl = (async () =>
      new Response("denied", { status: 403, statusText: "Forbidden" })) as unknown as typeof fetch
    await expectStorageError(
      downloadPresigned(httpRequest("http://storage.test/x?signature=do-not-log"), {
        fetchImpl,
        allowLocal: false,
      }),
      "HTTP 403",
    )

    const error = (await downloadPresigned(
      httpRequest("http://storage.test/x?signature=do-not-log"),
      { fetchImpl, allowLocal: false },
    ).catch(value => value)) as AlienError
    expect(JSON.stringify(error)).not.toContain("do-not-log")
    expect(error.context).toMatchObject({ url: "http://storage.test/x" })
  })

  it("does not retain a signed URL when fetch rejects", async () => {
    const secret = "do-not-log-fetch-token"
    const signedUrl = `http://storage.test/x?signature=${secret}`
    const fetchImpl = (async () => {
      throw new Error(`request to ${signedUrl} failed`)
    }) as unknown as typeof fetch

    const error = (await downloadPresigned(httpRequest(signedUrl), {
      fetchImpl,
      allowLocal: false,
    }).catch(value => value)) as AlienError

    expect(JSON.stringify(error)).not.toContain(secret)
    expect(error.context).toMatchObject({
      url: "http://storage.test/x",
      reason: "HTTP request failed before a response was received",
    })
  })

  it("reads a local file when allowLocal is true", async () => {
    const dir = await mkdtemp(join(tmpdir(), "presigned-"))
    const filePath = join(dir, "body.bin")
    await writeFile(filePath, new Uint8Array([9, 8, 7]))

    const bytes = await downloadPresigned(localRequest(filePath), { allowLocal: true })
    expect(Array.from(bytes)).toEqual([9, 8, 7])
  })

  it("refuses the local backend when allowLocal is false (sender policy)", async () => {
    await expectStorageError(
      downloadPresigned(localRequest("/tmp/whatever"), { allowLocal: false }),
      "Local storage backend not enabled",
    )
  })

  it("guards against path traversal in local paths", async () => {
    await expectStorageError(
      downloadPresigned(localRequest("/tmp/../etc/passwd"), { allowLocal: true }),
      "Path traversal",
    )
  })
})

describe("uploadPresigned", () => {
  it("uploads bytes over http and fails loudly on a non-2xx response", async () => {
    const seen: { method?: string; body?: unknown } = {}
    const okFetch = (async (_url: unknown, init?: RequestInit) => {
      seen.method = init?.method
      seen.body = init?.body
      return new Response(null, { status: 200 })
    }) as unknown as typeof fetch

    const request: PresignedRequest = {
      backend: { type: "http", url: "http://storage.test/up", method: "PUT", headers: {} },
      expiration: FUTURE,
      operation: "put",
      path: "bodies/abc",
    }
    await uploadPresigned(request, new Uint8Array([4, 5]), {
      fetchImpl: okFetch,
      allowLocal: false,
    })
    expect(seen.method).toBe("PUT")
    expect(Array.from(seen.body as Uint8Array)).toEqual([4, 5])

    const failFetch = (async () => new Response(null, { status: 500 })) as unknown as typeof fetch
    await expectStorageError(
      uploadPresigned(request, new Uint8Array([4, 5]), { fetchImpl: failFetch, allowLocal: false }),
      "status 500",
    )
  })

  it("writes a local file when allowLocal is true, with the traversal guard", async () => {
    const dir = await mkdtemp(join(tmpdir(), "presigned-"))
    const filePath = join(dir, "out.bin")
    const request: PresignedRequest = {
      backend: { type: "local", filePath, operation: "put" },
      expiration: FUTURE,
      operation: "put",
      path: "bodies/abc",
    }
    await uploadPresigned(request, new Uint8Array([6, 7]), { allowLocal: true })
    expect(Array.from(new Uint8Array(await readFile(filePath)))).toEqual([6, 7])

    const traversal: PresignedRequest = {
      backend: { type: "local", filePath: `${dir}/../evil.bin`, operation: "put" },
      expiration: FUTURE,
      operation: "put",
      path: "bodies/abc",
    }
    await expectStorageError(
      uploadPresigned(traversal, new Uint8Array([1]), { allowLocal: true }),
      "Path traversal",
    )
  })

  it("rejects an expired presigned upload before writing", async () => {
    const request: PresignedRequest = {
      backend: { type: "local", filePath: "/tmp/never-written.bin", operation: "put" },
      expiration: PAST,
      operation: "put",
      path: "bodies/abc",
    }
    await expectStorageError(
      uploadPresigned(request, new Uint8Array([1]), { allowLocal: true }),
      "expired",
    )
  })
})
