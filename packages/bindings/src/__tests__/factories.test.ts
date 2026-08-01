import { AlienError } from "@alienplatform/core"
import { describe, expect, it, vi } from "vitest"
import { createFactories } from "../factories.js"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawContainerHandle,
  RawKvHandle,
  RawPostgresConnection,
  RawPostgresHandle,
  RawQueueHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "../loader.js"

function unusedRemoteBindingsHandle(): NativeAddon["RemoteBindingsHandle"] {
  return {
    async forDeployment(): Promise<never> {
      throw new Error("unused")
    },
  }
}

/**
 * A fake addon that records every `BindingsHandle` construction and returns
 * trivial resource handles, so factory behavior can be exercised without the
 * real `.node`.
 */
/**
 * A raw addon connection for `sslmode`, matching what the napi `PostgresHandle`
 * returns: the connection string already carries the same mode.
 */
const TEST_CA = "-----BEGIN CERTIFICATE-----\ntest-root\n-----END CERTIFICATE-----"

function rawConnection(
  sslmode: unknown,
  caCertificates: unknown = typeof sslmode === "string" && sslmode.startsWith("verify-")
    ? [TEST_CA]
    : [],
): RawPostgresConnection {
  return {
    connectionString: `postgres://alien:pw@db.internal:5432/app?sslmode=${sslmode}`,
    host: "db.internal",
    port: 5432,
    database: "app",
    username: "alien",
    password: "pw",
    sslmode,
    caCertificates,
  }
}

/** Build an addon whose `postgres(name)` resolves to a handle returning `raw`. */
function addonForPostgres(raw: RawPostgresConnection): NativeAddon {
  class FakeBindingsHandle {
    async postgres(): Promise<RawPostgresHandle> {
      return { connection: () => raw }
    }
  }
  return {
    BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    RemoteBindingsHandle: unusedRemoteBindingsHandle(),
    version: () => "test",
  }
}

function fakeAddon(): { addon: NativeAddon; constructions: unknown[] } {
  const constructions: unknown[] = []

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
  const containerHandle: RawContainerHandle = {
    getInternalUrl: async () => "http://service.internal:8080",
    getPublicUrl: async () => null,
  }
  const postgresHandle: RawPostgresHandle = {
    connection: () => rawConnection("verify-full"),
  }

  const bindings: RawBindingsHandle = {
    storage: async () => storageHandle,
    kv: async () => kvHandle,
    queue: async () => queueHandle,
    vault: async () => vaultHandle,
    container: async () => containerHandle,
    postgres: async () => postgresHandle,
  }

  class FakeBindingsHandle {
    constructor() {
      constructions.push(undefined)
    }
    storage = bindings.storage
    kv = bindings.kv
    queue = bindings.queue
    vault = bindings.vault
    container = bindings.container
    postgres = bindings.postgres
  }

  return {
    addon: {
      BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
      RemoteBindingsHandle: unusedRemoteBindingsHandle(),
      version: () => "test",
    },
    constructions,
  }
}

/** Build an addon whose `kv(name)` resolves to a caller-supplied handle. */
function addonForKv(kvHandle: RawKvHandle): NativeAddon {
  class FakeBindingsHandle {
    async kv(): Promise<RawKvHandle> {
      return kvHandle
    }
    async storage() {
      throw new Error("unused")
    }
    async queue() {
      throw new Error("unused")
    }
    async vault() {
      throw new Error("unused")
    }
    async container() {
      throw new Error("unused")
    }
  }
  return {
    BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    RemoteBindingsHandle: unusedRemoteBindingsHandle(),
    version: () => "test",
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
    const { addon, constructions } = fakeAddon()
    const { storage } = createFactories(() => addon)

    const s = storage("files")
    await s.head("a")
    await s.head("b")

    expect(constructions).toHaveLength(1)
  })

  it("returns an independent handle per factory call", async () => {
    const { addon, constructions } = fakeAddon()
    const { storage } = createFactories(() => addon)

    await storage("files").head("a")
    await storage("files").head("b")

    expect(constructions).toHaveLength(2)
  })
})

describe("createFactories kv surface", () => {
  it("kv.get returns the raw value bytes over the napi get", async () => {
    const get = vi.fn(async () => Buffer.from("raw-bytes"))
    const kvHandle: RawKvHandle = {
      get,
      put: async () => true,
      delete: async () => {},
      exists: async () => false,
      scan: async () => ({ items: [] }),
    }
    const addon = addonForKv(kvHandle)
    const { kv } = createFactories(() => addon)

    const value = await kv("cache").get("k")

    expect(get).toHaveBeenCalledWith("k")
    expect(value).not.toBeNull()
    expect((value as Buffer).toString("utf8")).toBe("raw-bytes")
  })

  it("kv.get returns null when the key is absent", async () => {
    const kvHandle: RawKvHandle = {
      get: async () => null,
      put: async () => true,
      delete: async () => {},
      exists: async () => false,
      scan: async () => ({ items: [] }),
    }
    const { kv } = createFactories(() => addonForKv(kvHandle))

    expect(await kv("cache").get("missing")).toBeNull()
  })

  it("kv.setJson serializes the value as JSON before calling put", async () => {
    const put = vi.fn<RawKvHandle["put"]>(async () => true)
    const kvHandle: RawKvHandle = {
      get: async () => null,
      put,
      delete: async () => {},
      exists: async () => false,
      scan: async () => ({ items: [] }),
    }
    const { kv } = createFactories(() => addonForKv(kvHandle))

    const created = await kv("cache").setJson("k", { hello: "world" }, { ttl: 30 })

    expect(created).toBe(true)
    const firstCall = put.mock.calls[0]
    if (!firstCall) throw new Error("put was not called")
    const [key, buffer, ttl, ifNotExists] = firstCall
    expect(key).toBe("k")
    expect(buffer.toString("utf8")).toBe(JSON.stringify({ hello: "world" }))
    expect(ttl).toBe(30)
    expect(ifNotExists).toBeNull()
  })

  it("kv.scan surfaces items with both keys and values (no data discarded)", async () => {
    const kvHandle: RawKvHandle = {
      get: async () => null,
      put: async () => true,
      delete: async () => {},
      exists: async () => false,
      scan: async () => ({
        items: [
          { key: "a", value: Buffer.from("one") },
          { key: "b", value: Buffer.from("two") },
        ],
        nextCursor: "next",
      }),
    }
    const { kv } = createFactories(() => addonForKv(kvHandle))

    const page = await kv("cache").scan("prefix", 10, "cursor")

    expect(page.nextCursor).toBe("next")
    expect(page.items.map(item => item.key)).toEqual(["a", "b"])
    expect(page.items.map(item => item.value.toString("utf8"))).toEqual(["one", "two"])
  })
})

describe("createFactories method mapping", () => {
  it("serializes queue.send payloads as JSON via the bound queue handle", async () => {
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
      RemoteBindingsHandle: unusedRemoteBindingsHandle(),
      version: () => "test",
    }
    const { queue } = createFactories(() => addon)

    await queue("events").send({ hello: "world" })

    expect(sendJson).toHaveBeenCalledWith(JSON.stringify({ hello: "world" }))
  })

  it("exposes linked-container URLs through the lazy handle", async () => {
    const { addon } = fakeAddon()
    const { container } = createFactories(() => addon)

    await expect(container("database").getInternalUrl()).resolves.toBe(
      "http://service.internal:8080",
    )
    await expect(container("database").getPublicUrl()).resolves.toBeNull()
  })
})

describe("createFactories postgres surface", () => {
  it("passes every addon field through", async () => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection("verify-full")))

    const connection = await postgres("db").connection()

    expect(connection).toEqual({
      connectionString: "postgres://alien:pw@db.internal:5432/app?sslmode=verify-full",
      ssl: { ca: [TEST_CA], rejectUnauthorized: true },
      host: "db.internal",
      port: 5432,
      database: "app",
      username: "alien",
      password: "pw",
      sslmode: "verify-full",
    })
  })

  // `ssl` is what a node-postgres caller actually passes to the driver, so the mapping
  // from the Rust SslMode has to be exact.
  it.each([
    ["disable", false],
    ["verify-full", { ca: [TEST_CA], rejectUnauthorized: true }],
  ] as const)("derives ssl from sslmode %s", async (sslmode, expected) => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection(sslmode)))

    const connection = await postgres("db").connection()

    expect(connection.sslmode).toBe(sslmode)
    expect(connection.ssl).toEqual(expected)
  })

  it("uses system roots for verify-full when the addon supplies no CA", async () => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection("verify-full", [])))

    await expect(postgres("db").connection()).resolves.toMatchObject({
      ssl: { rejectUnauthorized: true },
      sslmode: "verify-full",
    })
  })

  it("keeps CA verification but skips hostname matching for Cloud SQL PSC IPs", async () => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection("verify-ca")))

    const connection = await postgres("db").connection()
    if (connection.sslmode !== "verify-ca") {
      throw new Error(`expected verify-ca, received ${connection.sslmode}`)
    }

    expect(connection.ssl.ca).toEqual([TEST_CA])
    expect(connection.ssl.rejectUnauthorized).toBe(true)
    expect(connection.ssl.checkServerIdentity).toBeTypeOf("function")
    expect(connection.ssl.checkServerIdentity()).toBeUndefined()
  })

  it("fails closed when a verified mode has no CA roots", async () => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection("verify-ca", [])))

    const error = await postgres("db")
      .connection()
      .catch((e: unknown) => e)

    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("INVALID_POSTGRES_TLS_CONFIG")
    expect((error as AlienError).retryable).toBe(false)
  })

  it("reports a malformed CA element as a TLS contract error", async () => {
    const { postgres } = createFactories(() =>
      addonForPostgres(rawConnection("verify-full", [TEST_CA, 42])),
    )

    const error = await postgres("db")
      .connection()
      .catch((e: unknown) => e)

    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("INVALID_POSTGRES_TLS_CONFIG")
    expect((error as AlienError).message).toContain("caCertificates[1] must be a string")
  })

  // An sslmode this wrapper doesn't know means wrapper/addon version skew. Silently
  // defaulting would hand back a connection with the wrong TLS posture, so it must throw.
  it("rejects an unknown sslmode from the addon rather than guessing a TLS posture", async () => {
    const { postgres } = createFactories(() => addonForPostgres(rawConnection("prefer")))

    const error = await postgres("db")
      .connection()
      .catch((e: unknown) => e)

    // The code is what a caller discriminates on: `guard` would otherwise flatten a
    // bare `Error` into the generic BINDINGS_ERROR fallback.
    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("UNKNOWN_POSTGRES_SSLMODE")
    expect((error as AlienError).retryable).toBe(false)
    expect((error as AlienError).message).toContain("prefer")
    expect((error as AlienError).message).toContain("disable, verify-ca, verify-full")
  })
})
