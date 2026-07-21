import { describe, expect, it, vi } from "vitest"
import { readRemoteStorageSmokeConfig, verifyRemoteStorage } from "./remote-storage-smoke-lib.mjs"

const object = "alien-e2e/remote-storage-smoke/test/payload.txt"

function fakeStorage() {
  const values = new Map()
  const put = vi.fn(async (path, data) => {
    values.set(path, Buffer.from(data))
  })
  const get = vi.fn(async path => {
    const value = values.get(path)
    if (!value) throw new Error(`missing ${path}`)
    return value
  })
  const head = vi.fn(async path => {
    const value = values.get(path)
    if (!value) throw new Error(`missing ${path}`)
    return { location: path, size: value.byteLength, lastModified: "2026-01-01T00:00:00Z" }
  })
  const list = vi.fn(async prefix =>
    [...values.entries()]
      .filter(([path]) => path.startsWith(prefix ?? ""))
      .map(([location, value]) => ({
        location,
        size: value.byteLength,
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
      deploymentId: "dep_123",
      storageBinding: "archive",
    })
  })

  it("checks every remote operation and verifies deletion", async () => {
    const fixture = fakeStorage()

    await verifyRemoteStorage(fixture.storage, object)

    expect(fixture.put).toHaveBeenCalledOnce()
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
