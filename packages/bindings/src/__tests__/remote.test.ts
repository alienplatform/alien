import { AlienError } from "@alienplatform/core"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawContainerHandle,
  RawKvHandle,
  RawQueueHandle,
  RawRemoteBindingsHandle,
  RawRemoteStorageHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "../loader.js"

const loadAddon = vi.hoisted(() => vi.fn<() => NativeAddon>())

vi.mock("../loader.js", async importOriginal => {
  const actual = await importOriginal<typeof import("../loader.js")>()
  return { ...actual, loadAddon }
})

import { Bindings } from "../remote.js"

function fakeRemoteAddon() {
  const head = vi.fn<RawRemoteStorageHandle["head"]>(async () => {
    throw new Error("unused")
  })
  const storage: RawRemoteStorageHandle = {
    get: async path => Buffer.from(path),
    put: async () => {},
    delete: async () => {},
    list: async () => [],
    head,
  }
  const resolveStorage = vi.fn<(name: string) => Promise<RawRemoteStorageHandle>>(
    async () => storage,
  )
  const localStorage: RawStorageHandle = {
    ...storage,
    copy: async () => {},
    signedUrl: async () => ({ url: "https://example.invalid", method: "GET", headers: {} }),
  }

  class FakeBindingsHandle implements RawBindingsHandle {
    async storage(): Promise<RawStorageHandle> {
      return localStorage
    }

    async kv(): Promise<RawKvHandle> {
      throw new Error("unused")
    }

    async queue(): Promise<RawQueueHandle> {
      throw new Error("unused")
    }

    async vault(): Promise<RawVaultHandle> {
      throw new Error("unused")
    }

    async container(): Promise<RawContainerHandle> {
      throw new Error("unused")
    }
  }

  class FakeRemoteBindingsHandle implements RawRemoteBindingsHandle {
    static forDeployment: (
      deploymentId: string,
      token: string,
      apiBaseUrl?: string,
    ) => Promise<RawRemoteBindingsHandle>

    storage = resolveStorage
  }

  const forRemoteDeployment = vi.fn<
    (deploymentId: string, token: string, apiBaseUrl?: string) => Promise<RawRemoteBindingsHandle>
  >(async () => new FakeRemoteBindingsHandle())
  FakeRemoteBindingsHandle.forDeployment = forRemoteDeployment

  return {
    addon: {
      BindingsHandle: FakeBindingsHandle,
      RemoteBindingsHandle: FakeRemoteBindingsHandle,
      version: () => "test",
    },
    forRemoteDeployment,
    resolveStorage,
    head,
  }
}

beforeEach(() => {
  loadAddon.mockReset()
})

describe("Bindings.forRemoteDeployment", () => {
  it("forwards discovery arguments and exposes only remote Storage", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)

    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
      apiBaseUrl: "https://api.example.com",
    })
    const storage = bindings.storage("archive")

    expect(loadAddon).toHaveBeenCalledTimes(1)
    expect(fixture.forRemoteDeployment).toHaveBeenCalledOnce()
    expect(fixture.forRemoteDeployment).toHaveBeenCalledWith(
      "dep_123",
      "token_123",
      "https://api.example.com",
    )
    expect("kv" in bindings).toBe(false)
    expect("queue" in bindings).toBe(false)
    expect("vault" in bindings).toBe(false)
    expect(Object.keys(storage).sort()).toEqual(["delete", "get", "head", "list", "put"])
  })

  it("reuses one native bindings handle and resolves each Storage handle lazily once", async () => {
    const fixture = fakeRemoteAddon()
    fixture.head.mockResolvedValue({
      location: "archive/a.txt",
      size: 1,
      lastModified: "2026-01-01T00:00:00Z",
    })
    loadAddon.mockReturnValue(fixture.addon)

    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })
    const archive = bindings.storage("archive")
    const logs = bindings.storage("logs")

    expect(fixture.resolveStorage).not.toHaveBeenCalled()
    expect(bindings.storage("archive")).toBe(archive)
    await archive.head("a.txt")
    await archive.get("a.txt")
    await logs.head("b.txt")

    expect(fixture.forRemoteDeployment).toHaveBeenCalledOnce()
    expect(fixture.resolveStorage.mock.calls).toEqual([["archive"], ["logs"]])
  })

  it("unwraps napi errors from discovery and Storage operations", async () => {
    const fixture = fakeRemoteAddon()
    const discoveryError = new Error(
      JSON.stringify({
        code: "REMOTE_BINDING_DENIED",
        message: "Remote binding access denied",
        retryable: false,
      }),
    )
    fixture.forRemoteDeployment.mockRejectedValueOnce(discoveryError)
    loadAddon.mockReturnValue(fixture.addon)

    const denied = Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })
    await expect(denied).rejects.toMatchObject({
      code: "REMOTE_BINDING_DENIED",
      message: "Remote binding access denied",
    })

    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })
    fixture.head.mockRejectedValueOnce(new Error("native transport failed"))
    const operation = bindings.storage("archive").head("a.txt")

    await expect(operation).rejects.toBeInstanceOf(AlienError)
    await expect(operation).rejects.toMatchObject({
      code: "BINDINGS_ERROR",
      message: "native transport failed",
    })
  })
})
