import { describe, expect, it, vi } from "vitest"
import { readRemoteStorageSmokeConfig, verifyRemoteStorage } from "./remote-storage-smoke-lib.mjs"

const object = "alien-e2e/remote-storage-smoke/test/payload.txt"

function fakeStorage() {
  const values = new Map()
  const put = vi.fn(async (path, data, options) => {
    values.set(path, {
      data: Buffer.from(data),
      attributes: options?.attributes ?? { metadata: {} },
    })
    return { eTag: "test-etag", version: "test-version" }
  })
  const get = vi.fn(async path => {
    const value = values.get(path)
    if (!value) throw new Error(`missing ${path}`)
    return {
      data: value.data,
      meta: {
        location: path,
        size: value.data.byteLength,
        lastModified: "2026-01-01T00:00:00Z",
      },
      attributes: value.attributes,
    }
  })
  const head = vi.fn(async path => {
    const value = values.get(path)
    if (!value) throw new Error(`missing ${path}`)
    return {
      meta: {
        location: path,
        size: value.data.byteLength,
        lastModified: "2026-01-01T00:00:00Z",
      },
      attributes: value.attributes,
    }
  })
  const list = vi.fn(async prefix =>
    [...values.entries()]
      .filter(([path]) => path.startsWith(prefix ?? ""))
      .map(([location, value]) => ({
        location,
        size: value.data.byteLength,
        lastModified: "2026-01-01T00:00:00Z",
      })),
  )
  const remove = vi.fn(async path => {
    values.delete(path)
  })
  return { storage: { put, get, head, list, delete: remove }, put, get, head, list, remove }
}

describe("remote Storage smoke", () => {
  it("reports every missing input together", () => {
    expect(() => readRemoteStorageSmokeConfig({ ALIEN_DEPLOYMENT_ID: " dep_123 " })).toThrow(
      "Missing required environment variables: ALIEN_API_URL, ALIEN_API_KEY, ALIEN_STORAGE_BINDING",
    )
  })

  it("reads and trims its public inputs", () => {
    expect(
      readRemoteStorageSmokeConfig({
        ALIEN_API_URL: " https://api.example.com ",
        ALIEN_API_KEY: " token_123 ",
        ALIEN_DEPLOYMENT_ID: " dep_123 ",
        ALIEN_STORAGE_BINDING: " archive ",
      }),
    ).toEqual({
      apiUrl: "https://api.example.com",
      apiKey: "token_123",
      selector: { type: "deployment", deploymentId: "dep_123" },
      storageBinding: "archive",
    })
  })

  it("accepts a stable Project and external-ID selector", () => {
    expect(
      readRemoteStorageSmokeConfig({
        ALIEN_API_URL: "https://api.example.com",
        ALIEN_API_KEY: "token_123",
        ALIEN_PROJECT: "customer-files",
        ALIEN_EXTERNAL_ID: "customer_123",
        ALIEN_STORAGE_BINDING: "storage",
      }),
    ).toMatchObject({
      selector: {
        type: "customer",
        project: "customer-files",
        externalId: "customer_123",
      },
    })
  })

  it("rejects missing or ambiguous selectors", () => {
    const common = {
      ALIEN_API_URL: "https://api.example.com",
      ALIEN_API_KEY: "token_123",
      ALIEN_STORAGE_BINDING: "storage",
    }
    expect(() => readRemoteStorageSmokeConfig(common)).toThrow(
      "Set either ALIEN_DEPLOYMENT_ID or both ALIEN_PROJECT and ALIEN_EXTERNAL_ID",
    )
    expect(() =>
      readRemoteStorageSmokeConfig({
        ...common,
        ALIEN_DEPLOYMENT_ID: "dep_123",
        ALIEN_PROJECT: "customer-files",
        ALIEN_EXTERNAL_ID: "customer_123",
      }),
    ).toThrow("Set either ALIEN_DEPLOYMENT_ID or both ALIEN_PROJECT and ALIEN_EXTERNAL_ID")
  })

  it("checks every remote operation and verifies deletion", async () => {
    const fixture = fakeStorage()

    await verifyRemoteStorage(fixture.storage, object)

    expect(fixture.put).toHaveBeenCalledOnce()
    expect(fixture.put).toHaveBeenCalledWith(object, expect.any(Buffer), {
      attributes: {
        contentType: "application/octet-stream",
        contentDisposition: 'attachment; filename="payload.txt"',
        cacheControl: "private, max-age=60",
        metadata: { source: "remote-storage-smoke" },
      },
    })
    expect(fixture.get).toHaveBeenCalledWith(object)
    expect(fixture.head).toHaveBeenCalledOnce()
    expect(fixture.list).toHaveBeenCalledTimes(2)
    expect(fixture.list).toHaveBeenCalledWith("alien-e2e/remote-storage-smoke/test/")
    expect(fixture.remove).toHaveBeenCalledOnce()
  })

  it("fails when delete leaves the object visible", async () => {
    const fixture = fakeStorage()
    fixture.remove.mockImplementationOnce(async () => {})

    await expect(verifyRemoteStorage(fixture.storage, object)).rejects.toThrow(
      "deleted object remained in list",
    )
  })

  it("deletes the object when verification fails", async () => {
    const fixture = fakeStorage()
    fixture.get.mockRejectedValueOnce(new Error("download failed"))

    await expect(verifyRemoteStorage(fixture.storage, object)).rejects.toThrow("download failed")
    expect(fixture.remove).toHaveBeenCalledWith(object)
  })

  it("preserves both verification and cleanup failures", async () => {
    const fixture = fakeStorage()
    const verificationError = new Error("download failed")
    const cleanupError = new Error("delete failed")
    fixture.get.mockRejectedValueOnce(verificationError)
    fixture.remove.mockRejectedValueOnce(cleanupError)

    await expect(verifyRemoteStorage(fixture.storage, object)).rejects.toMatchObject({
      errors: [verificationError, cleanupError],
    })
  })
})
