import { AlienError } from "@alienplatform/core"
import { describe, expect, it, vi } from "vitest"
import { createFactories } from "../factories.js"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawKvHandle,
  RawQueueHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "../loader.js"

/**
 * A fake addon that records every `BindingsHandle` env argument and returns
 * trivial resource handles, so factory behavior can be exercised without the
 * real `.node`.
 */
function fakeAddon(): { addon: NativeAddon; envs: (Record<string, string> | null | undefined)[] } {
  const envs: (Record<string, string> | null | undefined)[] = []

  const storageHandle: RawStorageHandle = {
    get: async () => Buffer.from("x"),
    put: async () => {},
    delete: async () => {},
    list: async () => [],
    head: async () => ({ location: "p", size: 0, lastModified: "" }),
    copy: async () => {},
    signedUrl: async () => ({ url: "u", method: "GET", headers: {} }),
  }
  const kvHandle: RawKvHandle = {
    get: async () => null,
    put: async () => true,
    delete: async () => {},
    exists: async () => false,
    scan: async () => ({ items: [] }),
  }
  const queueHandle: RawQueueHandle = {
    sendJson: async () => {},
    sendText: async () => {},
    receive: async () => [],
    ack: async () => {},
    nack: async () => {},
    purge: async () => {},
  }
  const vaultHandle: RawVaultHandle = {
    getSecret: async () => "s",
    setSecret: async () => {},
    deleteSecret: async () => {},
    listSecrets: async () => [],
  }

  const bindings: RawBindingsHandle = {
    storage: async () => storageHandle,
    kv: async () => kvHandle,
    queue: async () => queueHandle,
    vault: async () => vaultHandle,
  }

  class FakeBindingsHandle {
    constructor(env?: Record<string, string> | null) {
      envs.push(env)
    }
    storage = bindings.storage
    kv = bindings.kv
    queue = bindings.queue
    vault = bindings.vault
  }

  return {
    addon: {
      BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
      version: () => "test",
    },
    envs,
  }
}

describe("createFactories laziness", () => {
  it("constructs factories without loading the addon; loads only on first op", async () => {
    const getAddon = vi.fn<() => NativeAddon>(() => {
      throw new Error("addon unavailable")
    })
    const { storage } = createFactories(getAddon)

    // Building the handle must not touch the addon — this is what justifies
    // sideEffects: false.
    const s = storage("files")
    expect(getAddon).not.toHaveBeenCalled()

    // The first operation triggers the load, which here fails; the raw error is
    // translated to an AlienError.
    await expect(s.head("a")).rejects.toBeInstanceOf(AlienError)
    expect(getAddon).toHaveBeenCalledTimes(1)
  })

  it("materializes the BindingsHandle once and caches it across operations", async () => {
    const { addon, envs } = fakeAddon()
    const { storage } = createFactories(() => addon)

    const s = storage("files")
    await s.head("a")
    await s.head("b")

    expect(envs).toHaveLength(1)
  })

  it("returns an independent handle per factory call", async () => {
    const { addon, envs } = fakeAddon()
    const { storage } = createFactories(() => addon)

    await storage("files").head("a")
    await storage("files").head("b")

    expect(envs).toHaveLength(2)
  })
})

describe("createFactories env filtering", () => {
  it("drops undefined env values before crossing into the addon", async () => {
    const { addon, envs } = fakeAddon()
    const { storage } = createFactories(() => addon)

    await storage("files", {
      env: { KEEP: "1", DROP: undefined, ALSO_KEEP: "2" },
    }).head("a")

    expect(envs[0]).toEqual({ KEEP: "1", ALSO_KEEP: "2" })
  })

  it("passes undefined when no env override is given", async () => {
    const { addon, envs } = fakeAddon()
    const { kv } = createFactories(() => addon)

    await kv("cache").exists("k")

    expect(envs[0]).toBeUndefined()
  })
})

describe("createFactories method mapping", () => {
  it("serializes queue.send payloads as JSON via sendJson using the binding name", async () => {
    const sendJson = vi.fn(async () => {})
    const queueHandle: RawQueueHandle = {
      sendJson,
      sendText: async () => {},
      receive: async () => [],
      ack: async () => {},
      nack: async () => {},
      purge: async () => {},
    }
    class FakeBindingsHandle {
      async queue(): Promise<RawQueueHandle> {
        return queueHandle
      }
      async storage() {
        throw new Error("unused")
      }
      async kv() {
        throw new Error("unused")
      }
      async vault() {
        throw new Error("unused")
      }
    }
    const addon = {
      BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
      version: () => "test",
    }
    const { queue } = createFactories(() => addon)

    await queue("events").send({ hello: "world" })

    expect(sendJson).toHaveBeenCalledWith("events", JSON.stringify({ hello: "world" }))
  })
})
