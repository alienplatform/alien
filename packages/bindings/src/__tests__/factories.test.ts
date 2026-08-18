import { AlienError } from "@alienplatform/core"
import { describe, expect, it, vi } from "vitest"
import { createFactories } from "../factories.js"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawCommandFrame,
  RawCommandStreamHandle,
  RawContainerHandle,
  RawKeyHandle,
  RawKvHandle,
  RawPostgresConnection,
  RawPostgresHandle,
  RawQueueHandle,
  RawSandboxHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "../loader.js"
import type { CommandFrame } from "../types.js"

function unusedRemoteBindingsHandle(): NativeAddon["RemoteBindingsHandle"] {
  return {
    async forCustomer(): Promise<never> {
      throw new Error("unused")
    },
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
    get: async () => ({
      data: Buffer.from("x"),
      meta: { location: "p", size: 1, lastModified: "" },
      attributes: { metadata: {} },
    }),
    put: async () => ({}),
    delete: async () => {},
    list: async () => [],
    head: async () => ({
      meta: { location: "p", size: 0, lastModified: "" },
      attributes: { metadata: {} },
    }),
    copy: async () => {},
    signedUrl: async () => ({ url: "u", method: "GET", headers: {} }),
  }
  const kvHandle: RawKvHandle = {
    get: async () => null,
    put: async () => true,
    delete: async () => true,
    exists: async () => false,
    scan: async () => ({ items: [] }),
  }
  const keyHandle: RawKeyHandle = {
    encrypt: async plaintext => plaintext,
    decrypt: async ciphertext => ciphertext,
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

  const sandboxHandle: RawSandboxHandle = {
    capabilities: () => ["reconnect"],
    create: async sessionId => ({ sessionId: sessionId ?? "s1", state: "running", generation: 1 }),
    get: async sessionId => ({ sessionId, state: "running", generation: 1 }),
    getOrCreate: async sessionId => ({
      sessionId: sessionId ?? "s1",
      state: "running",
      generation: 1,
    }),
    list: async () => [],
    runCommand: async () => {
      const frames: RawCommandFrame[] = [
        { kind: "stdout", seq: 0, data: Buffer.from("hello\n") },
        { kind: "stderr", seq: 1, data: Buffer.from("problem\n") },
        { kind: "exit", exitCode: 3, truncated: false },
      ]
      let index = 0
      return { next: async () => frames[index++] ?? null, close: async () => {} }
    },
    readFile: async () => Buffer.from("contents"),
    writeFile: async () => {},
    mkdir: async () => {},
    suspend: async () => {},
    resume: async () => {},
    terminate: async () => {},
  }

  const bindings: RawBindingsHandle = {
    storage: async () => storageHandle,
    key: async () => keyHandle,
    kv: async () => kvHandle,
    queue: async () => queueHandle,
    vault: async () => vaultHandle,
    container: async () => containerHandle,
    postgres: async () => postgresHandle,
    sandbox: async () => sandboxHandle,
  }

  class FakeBindingsHandle {
    constructor() {
      constructions.push(undefined)
    }
    storage = bindings.storage
    key = bindings.key
    kv = bindings.kv
    queue = bindings.queue
    vault = bindings.vault
    container = bindings.container
    postgres = bindings.postgres
    sandbox = bindings.sandbox
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

/** Build an addon whose `storage(name)` resolves to a caller-supplied handle. */
function addonForStorage(storageHandle: RawStorageHandle): NativeAddon {
  class FakeBindingsHandle {
    async storage(): Promise<RawStorageHandle> {
      return storageHandle
    }
  }
  return {
    BindingsHandle: FakeBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    RemoteBindingsHandle: unusedRemoteBindingsHandle(),
    version: () => "test",
  }
}

function addonForKey(keyHandle: RawKeyHandle): NativeAddon {
  class FakeBindingsHandle {
    async key(): Promise<RawKeyHandle> {
      return keyHandle
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
  it("kv.get returns the value and opaque version over the napi get", async () => {
    const get = vi.fn(async () => ({
      key: "k",
      value: Buffer.from("raw-bytes"),
      version: "opaque",
    }))
    const kvHandle: RawKvHandle = {
      get,
      put: async () => true,
      delete: async () => true,
      exists: async () => false,
      scan: async () => ({ items: [] }),
    }
    const addon = addonForKv(kvHandle)
    const { kv } = createFactories(() => addon)

    const value = await kv("cache").get("k")

    expect(get).toHaveBeenCalledWith("k")
    expect(value).not.toBeNull()
    expect(value?.value.toString("utf8")).toBe("raw-bytes")
    expect(value?.version).toBe("opaque")
  })

  it("kv.get returns null when the key is absent", async () => {
    const kvHandle: RawKvHandle = {
      get: async () => null,
      put: async () => true,
      delete: async () => true,
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
      delete: async () => true,
      exists: async () => false,
      scan: async () => ({ items: [] }),
    }
    const { kv } = createFactories(() => addonForKv(kvHandle))

    const created = await kv("cache").setJson("k", { hello: "world" }, { ttl: 30 })

    expect(created).toBe(true)
    const firstCall = put.mock.calls[0]
    if (!firstCall) throw new Error("put was not called")
    const [key, buffer, ttl, condition, version] = firstCall
    expect(key).toBe("k")
    expect(buffer.toString("utf8")).toBe(JSON.stringify({ hello: "world" }))
    expect(ttl).toBe(30)
    expect(condition).toBeNull()
    expect(version).toBeNull()
  })

  it("kv.scan surfaces items with both keys and values (no data discarded)", async () => {
    const kvHandle: RawKvHandle = {
      get: async () => null,
      put: async () => true,
      delete: async () => true,
      exists: async () => false,
      scan: async () => ({
        items: [
          { key: "a", value: Buffer.from("one"), version: "v1" },
          { key: "b", value: Buffer.from("two"), version: "v2" },
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

describe("createFactories key surface", () => {
  it("forwards bytes and authenticated context without changing either", async () => {
    const encrypt = vi.fn<RawKeyHandle["encrypt"]>(async plaintext => plaintext)
    const decrypt = vi.fn<RawKeyHandle["decrypt"]>(async ciphertext => ciphertext)
    const { key } = createFactories(() => addonForKey({ encrypt, decrypt }))
    const plaintext = new Uint8Array([0, 1, 2, 255])
    const context = { project: "example", purpose: "root" }

    const ciphertext = await key("customer-key").encrypt(plaintext, { context })
    await key("customer-key").decrypt(ciphertext, { context })

    expect(encrypt).toHaveBeenCalledWith(Buffer.from(plaintext), context)
    expect(decrypt).toHaveBeenCalledWith(Buffer.from(plaintext), context)
  })
})

describe("createFactories method mapping", () => {
  it("returns structured storage reads without discarding metadata or attributes", async () => {
    const result = {
      data: Buffer.from("hello"),
      meta: {
        location: "notes/note.txt",
        size: 5,
        lastModified: "2026-08-02T00:00:00Z",
        eTag: "etag-123",
        version: "version-456",
      },
      attributes: {
        contentType: "text/plain",
        storageClass: "STANDARD",
        metadata: { source: "upload" },
      },
    }
    const storageHandle: RawStorageHandle = {
      get: async () => result,
      put: async () => ({}),
      delete: async () => {},
      list: async () => [],
      head: async () => ({ meta: result.meta, attributes: result.attributes }),
      copy: async () => {},
      signedUrl: async () => ({ url: "u", method: "GET", headers: {} }),
    }
    const { storage } = createFactories(() => addonForStorage(storageHandle))

    await expect(storage("files").get("notes/note.txt")).resolves.toEqual(result)
    await expect(storage("files").head("notes/note.txt")).resolves.toEqual({
      meta: result.meta,
      attributes: result.attributes,
    })
  })

  it("forwards storage object attributes and converts Uint8Array data", async () => {
    const put = vi.fn<RawStorageHandle["put"]>(async () => ({
      eTag: "etag-123",
      version: "version-456",
    }))
    const storageHandle: RawStorageHandle = {
      get: async () => ({
        data: Buffer.from("x"),
        meta: { location: "p", size: 1, lastModified: "" },
        attributes: { metadata: {} },
      }),
      put,
      delete: async () => {},
      list: async () => [],
      head: async () => ({
        meta: { location: "p", size: 0, lastModified: "" },
        attributes: { metadata: {} },
      }),
      copy: async () => {},
      signedUrl: async () => ({ url: "u", method: "GET", headers: {} }),
    }
    const { storage } = createFactories(() => addonForStorage(storageHandle))
    const options = {
      attributes: {
        contentType: "text/plain",
        contentDisposition: 'attachment; filename="note.txt"',
        contentEncoding: "gzip",
        contentLanguage: "en-US",
        cacheControl: "private, max-age=60",
        metadata: { source: "upload", checksum: "abc123" },
      },
    }

    const result = await storage("files").put("notes/note.txt", new Uint8Array([1, 2, 3]), options)

    expect(put).toHaveBeenCalledOnce()
    const firstCall = put.mock.calls[0]
    if (!firstCall) throw new Error("put was not called")
    expect(firstCall[0]).toBe("notes/note.txt")
    expect(firstCall[1]).toEqual(Buffer.from([1, 2, 3]))
    expect(firstCall[2]).toEqual(options)
    expect(result).toEqual({ eTag: "etag-123", version: "version-456" })
  })

  it("preserves two-argument storage puts", async () => {
    const put = vi.fn<RawStorageHandle["put"]>(async () => ({}))
    const storageHandle: RawStorageHandle = {
      get: async () => ({
        data: Buffer.from("x"),
        meta: { location: "p", size: 1, lastModified: "" },
        attributes: { metadata: {} },
      }),
      put,
      delete: async () => {},
      list: async () => [],
      head: async () => ({
        meta: { location: "p", size: 0, lastModified: "" },
        attributes: { metadata: {} },
      }),
      copy: async () => {},
      signedUrl: async () => ({ url: "u", method: "GET", headers: {} }),
    }
    const { storage } = createFactories(() => addonForStorage(storageHandle))

    await storage("files").put("note.txt", Buffer.from("hello"))

    expect(put).toHaveBeenCalledWith("note.txt", Buffer.from("hello"), null)
  })

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

describe("sandbox streaming", () => {
  it("yields frames one at a time and ends after the terminal frame", async () => {
    const { addon } = fakeAddon()
    const { sandbox } = createFactories(() => addon)

    const seen: string[] = []
    for await (const frame of sandbox("sbx").runCommand("s1", ["/bin/echo"], {
      deadlineMs: 10_000,
    })) {
      seen.push(
        frame.kind === "exit" ? `exit:${frame.exitCode}` : frame.data.toString("utf8").trim(),
      )
    }

    expect(seen).toEqual(["hello", "problem", "exit:3"])
  })

  it("starts one command when two pulls race the setup", async () => {
    const { addon } = fakeAddon()
    let opened = 0
    let finishSetup: () => void = () => {}
    let setupStarted: () => void = () => {}
    const setup = new Promise<void>(resolve => {
      finishSetup = resolve
    })
    const started = new Promise<void>(resolve => {
      setupStarted = resolve
    })

    class CountingBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async (...args: Parameters<RawSandboxHandle["runCommand"]>) => {
            opened += 1
            setupStarted()
            await setup
            return await inner.runCommand(...args)
          },
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: CountingBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    const first = iterator.next()
    const second = iterator.next()

    await started
    expect(opened).toBe(1)

    finishSetup()
    await expect(Promise.all([first, second])).resolves.toEqual([
      {
        done: false,
        value: { kind: "stdout", seq: 0, data: Buffer.from("hello\n") },
      },
      {
        done: false,
        value: { kind: "stderr", seq: 1, data: Buffer.from("problem\n") },
      },
    ])

    expect(opened).toBe(1)
  })

  it("does not start a command when the caller never pulls", async () => {
    const { addon } = fakeAddon()
    let opened = 0

    class CountingBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: (...args: Parameters<RawSandboxHandle["runCommand"]>) => {
            opened += 1
            return inner.runCommand(...args)
          },
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: CountingBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    sandbox("sbx").runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 })
    await new Promise(resolve => setTimeout(resolve, 0))

    expect(opened).toBe(0)
  })

  it("closes a parked read before return waits for it", async () => {
    const { addon } = fakeAddon()
    let closed = 0
    let readStarted: () => void = () => {}
    let finishRead: (frame: RawCommandFrame | null) => void = () => {}
    const reading = new Promise<void>(resolve => {
      readStarted = resolve
    })
    const parked = new Promise<RawCommandFrame | null>(resolve => {
      finishRead = resolve
    })

    class SilentBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => {
            return {
              next: () => {
                readStarted()
                return parked
              },
              close: async () => {
                closed += 1
                finishRead(null)
              },
            }
          },
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: SilentBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/sleep"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    const pull = iterator.next()
    await reading

    const returned = iterator.return?.()
    if (!returned) throw new Error("iterator return is missing")

    expect(closed).toBe(1)
    await expect(returned).resolves.toEqual({ done: true, value: undefined })
    await expect(pull).resolves.toEqual({ done: true, value: undefined })
  })

  it("closes a command that finishes setup after return", async () => {
    const { addon } = fakeAddon()
    let allowSetup: () => void = () => {}
    let setupStarted: () => void = () => {}
    const setup = new Promise<void>(resolve => {
      allowSetup = resolve
    })
    const started = new Promise<void>(resolve => {
      setupStarted = resolve
    })
    let closed = 0
    const next = vi.fn<RawCommandStreamHandle["next"]>(async () => ({
      kind: "stdout",
      seq: 0,
      data: Buffer.from("too late"),
    }))
    const runCommand = vi.fn<RawSandboxHandle["runCommand"]>(async () => {
      setupStarted()
      await setup
      return {
        next,
        close: async () => {
          closed += 1
        },
      }
    })

    class SlowStartBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return { ...inner, runCommand }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: SlowStartBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/sleep"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    const pull = iterator.next()
    await started

    // return() must not resolve while the command it is cancelling is still being started:
    // a caller told the cancel is complete would otherwise leave a command about to run.
    let returned = false
    const returning = iterator.return?.().then(result => {
      returned = true
      return result
    })
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(returned).toBe(false)
    expect(closed).toBe(0)

    allowSetup()

    await expect(returning).resolves.toEqual({ done: true, value: undefined })
    expect(closed).toBe(1)
    await expect(pull).resolves.toEqual({ done: true, value: undefined })
    expect(next).not.toHaveBeenCalled()
    await expect(iterator.next()).resolves.toEqual({ done: true, value: undefined })
    expect(runCommand).toHaveBeenCalledTimes(1)
    expect(closed).toBe(1)
  })

  // A pull in flight when the caller cancels only observes the cancel. The close failure belongs
  // to the throw() that caused it, and must not surface a second time on the pending pull.
  it("completes a pending pull as done when throw() closes and the close fails", async () => {
    const { addon } = fakeAddon()
    let unpark: (frame: RawCommandFrame | null) => void = () => {}
    const parked = new Promise<RawCommandFrame | null>(resolve => {
      unpark = resolve
    })

    class ParkedFailingCloseBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: () => parked,
            close: async () => {
              unpark(null)
              throw new Error("close exploded")
            },
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: ParkedFailingCloseBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/sleep"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    const pending = iterator.next()
    await new Promise(resolve => setTimeout(resolve, 0))

    const original = new Error("caller gave up")
    await expect(iterator.throw?.(original)).rejects.toBe(original)
    await expect(pending).resolves.toEqual({ done: true, value: undefined })
  })

  // Same rule where the stream itself fails: the read error ended iteration, and the close
  // that follows must not overwrite it even if that close also fails.
  it("reports the read error, not the close error, when both fail", async () => {
    const { addon } = fakeAddon()
    let closeAttempts = 0

    class DoubleFaultBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: async () => {
              throw new Error("read exploded")
            },
            close: async () => {
              closeAttempts += 1
              throw new Error("close exploded")
            },
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: DoubleFaultBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()

    await expect(iterator.next()).rejects.toThrow("read exploded")
    expect(closeAttempts).toBe(1)
  })

  // The caller's error is what iteration failed on; a close that also fails must not replace it.
  it("rethrows the caller's error from throw() even when the native close fails", async () => {
    const { addon } = fakeAddon()
    let closeAttempts = 0

    class FailingCloseBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: async () => ({ kind: "stdout", seq: 0, data: Buffer.from("x") }),
            close: async () => {
              closeAttempts += 1
              throw new Error("close exploded")
            },
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: FailingCloseBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    await iterator.next()

    const original = new Error("caller gave up")
    await expect(iterator.throw?.(original)).rejects.toBe(original)
    expect(closeAttempts).toBe(1)
  })

  it("serializes overlapping pulls after the stream opens", async () => {
    const { addon } = fakeAddon()
    let active = false
    let reentered = false
    let finishSecond: () => void = () => {}
    let secondStarted: () => void = () => {}
    const second = new Promise<void>(resolve => {
      finishSecond = resolve
    })
    const started = new Promise<void>(resolve => {
      secondStarted = resolve
    })
    const close = vi.fn<RawCommandStreamHandle["close"]>(async () => {})
    const next = vi.fn<RawCommandStreamHandle["next"]>(async () => {
      const call = next.mock.calls.length
      if (active) reentered = true
      active = true
      try {
        if (call === 1) {
          return { kind: "stdout", seq: 0, data: Buffer.from("first") }
        }
        if (call === 2) {
          secondStarted()
          await second
          return { kind: "stderr", seq: 1, data: Buffer.from("second") }
        }
        return { kind: "exit", exitCode: 0, truncated: false }
      } finally {
        active = false
      }
    })

    class SerializedBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({ next, close }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: SerializedBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 10_000 })
      [Symbol.asyncIterator]()
    await expect(iterator.next()).resolves.toEqual({
      done: false,
      value: { kind: "stdout", seq: 0, data: Buffer.from("first") },
    })

    const secondPull = iterator.next()
    await started
    const thirdPull = iterator.next()
    await Promise.resolve()

    expect(next).toHaveBeenCalledTimes(2)
    expect(reentered).toBe(false)

    finishSecond()

    await expect(secondPull).resolves.toEqual({
      done: false,
      value: { kind: "stderr", seq: 1, data: Buffer.from("second") },
    })
    await expect(thirdPull).resolves.toEqual({
      done: false,
      value: { kind: "exit", exitCode: 0, truncated: false },
    })
    await iterator.return?.()

    expect(reentered).toBe(false)
    expect(close).toHaveBeenCalledTimes(1)
  })

  it("finishes after setup rejects without retaining the rejection", async () => {
    const { addon } = fakeAddon()
    const nativeError = new Error(
      JSON.stringify({
        code: "SANDBOX_SETUP_FAILED",
        message: "sandbox setup failed",
        retryable: true,
        internal: false,
      }),
    )
    const runCommand = vi.fn<RawSandboxHandle["runCommand"]>(async () => {
      throw nativeError
    })

    class FailingBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return { ...inner, runCommand }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: FailingBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 })
      [Symbol.asyncIterator]()
    const error = await iterator.next().catch((caught: unknown) => caught)

    expect(error).not.toBe(nativeError)
    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("SANDBOX_SETUP_FAILED")
    await expect(iterator.next()).resolves.toEqual({ done: true, value: undefined })
    await expect(iterator.return?.()).resolves.toEqual({ done: true, value: undefined })
    expect(runCommand).toHaveBeenCalledTimes(1)
  })

  it("closes the stream and unwraps a rejected native read", async () => {
    const { addon } = fakeAddon()
    const nativeError = new Error(
      JSON.stringify({
        code: "SANDBOX_READ_FAILED",
        message: "sandbox read failed",
        retryable: false,
        internal: false,
      }),
    )
    const close = vi.fn<RawCommandStreamHandle["close"]>(async () => {})

    class FailingReadBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: async () => {
              throw nativeError
            },
            close,
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: FailingReadBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 })
      [Symbol.asyncIterator]()
    const error = await iterator.next().catch((caught: unknown) => caught)

    expect(error).not.toBe(nativeError)
    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("SANDBOX_READ_FAILED")
    expect(close).toHaveBeenCalledTimes(1)
    await expect(iterator.next()).resolves.toEqual({ done: true, value: undefined })
  })

  it("closes the stream when frame conversion rejects", async () => {
    const { addon } = fakeAddon()
    const close = vi.fn<RawCommandStreamHandle["close"]>(async () => {})

    class UnknownFrameBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: async () => ({ kind: "future-output", seq: 0, data: Buffer.from("unknown") }),
            close,
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: UnknownFrameBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 })
      [Symbol.asyncIterator]()
    const error = await iterator.next().catch((caught: unknown) => caught)

    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("UNKNOWN_SANDBOX_VALUE")
    expect(close).toHaveBeenCalledTimes(1)
    await expect(iterator.next()).resolves.toEqual({ done: true, value: undefined })
  })

  it("drops a buffered frame returned while a parked read is closing", async () => {
    const { addon } = fakeAddon()
    let readStarted: () => void = () => {}
    let finishRead: (frame: RawCommandFrame | null) => void = () => {}
    let closed = false
    const reading = new Promise<void>(resolve => {
      readStarted = resolve
    })
    const parked = new Promise<RawCommandFrame | null>(resolve => {
      finishRead = resolve
    })
    const close = vi.fn<RawCommandStreamHandle["close"]>(async () => {
      if (closed) return
      closed = true
      finishRead({ kind: "stdout", seq: 0, data: Buffer.from("buffered") })
    })

    class BufferedFrameBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: () => {
              readStarted()
              return parked
            },
            close,
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: BufferedFrameBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/sleep"], { deadlineMs: 1000 })
      [Symbol.asyncIterator]()
    const pull = iterator.next()
    await reading

    await expect(iterator.return?.()).resolves.toEqual({ done: true, value: undefined })
    await expect(pull).resolves.toEqual({ done: true, value: undefined })
    expect(close).toHaveBeenCalled()
  })

  it("rejects return when the native close fails", async () => {
    const { addon } = fakeAddon()
    const nativeError = new Error(
      JSON.stringify({
        code: "SANDBOX_CLOSE_FAILED",
        message: "sandbox close failed",
        retryable: false,
        internal: false,
      }),
    )
    const close = vi.fn<RawCommandStreamHandle["close"]>(async () => {
      throw nativeError
    })

    class FailingCloseBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async () => ({
            next: async () => ({ kind: "stdout", seq: 0, data: Buffer.from("started") }),
            close,
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: FailingCloseBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    const iterator = sandbox("sbx")
      .runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 })
      [Symbol.asyncIterator]()
    await iterator.next()
    const error = await iterator.return?.().catch((caught: unknown) => caught)

    expect(error).not.toBe(nativeError)
    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("SANDBOX_CLOSE_FAILED")
    expect(close).toHaveBeenCalledTimes(1)
    await expect(iterator.next()).resolves.toEqual({ done: true, value: undefined })
  })

  it.each([
    [
      "the loop is broken out of",
      async (frames: AsyncIterable<CommandFrame>) => {
        for await (const _ of frames) break
      },
    ],
    [
      "the loop throws",
      async (frames: AsyncIterable<CommandFrame>) => {
        await expect(
          (async () => {
            for await (const _ of frames) throw new Error("caller gave up")
          })(),
        ).rejects.toThrow("caller gave up")
      },
    ],
    [
      "the command ends on its own",
      async (frames: AsyncIterable<CommandFrame>) => {
        for await (const _ of frames);
      },
    ],
  ])("closes the stream when %s", async (_case, consume) => {
    const { addon } = fakeAddon()
    let closed = 0

    class ClosingBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          runCommand: async (...args: Parameters<RawSandboxHandle["runCommand"]>) => ({
            ...(await inner.runCommand(...args)),
            close: async () => {
              closed += 1
            },
          }),
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: ClosingBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    await consume(sandbox("sbx").runCommand("s1", ["/bin/echo"], { deadlineMs: 1000 }))

    expect(closed).toBe(1)
  })

  it("passes env through to the addon on create and on a command", async () => {
    const { addon } = fakeAddon()
    const seen: Array<Record<string, string> | null | undefined> = []

    class RecordingBindingsHandle {
      async sandbox(name: string): Promise<RawSandboxHandle> {
        const inner = await new addon.BindingsHandle().sandbox(name)
        return {
          ...inner,
          create: (sessionId, tenantKey, env) => {
            seen.push(env)
            return inner.create(sessionId, tenantKey, env)
          },
          runCommand: (sessionId, command, deadlineMs, workingDirectory, env) => {
            seen.push(env)
            return inner.runCommand(sessionId, command, deadlineMs, workingDirectory, env)
          },
        }
      }
    }

    const { sandbox } = createFactories(() => ({
      ...addon,
      BindingsHandle: RecordingBindingsHandle as unknown as NativeAddon["BindingsHandle"],
    }))

    await sandbox("sbx").create({ env: { TOKEN: "s3cret" } })
    for await (const _ of sandbox("sbx").runCommand("s1", ["/bin/echo"], {
      deadlineMs: 1000,
      env: { EXTRA: "1" },
    }));

    expect(seen).toEqual([{ TOKEN: "s3cret" }, { EXTRA: "1" }])
  })

  it("writes files one call per path and reads them back as bytes", async () => {
    const { addon } = fakeAddon()
    const { sandbox } = createFactories(() => addon)

    await sandbox("sbx").writeFiles("s1", { "a.txt": "text", "b.bin": Buffer.from([1, 2]) })
    const read = await sandbox("sbx").readFile("s1", "a.txt")

    expect(Buffer.isBuffer(read)).toBe(true)
  })
})
