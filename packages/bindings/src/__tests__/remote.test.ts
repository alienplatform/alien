import { AlienError } from "@alienplatform/core"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawContainerHandle,
  RawKeyHandle,
  RawKvHandle,
  RawPostgresHandle,
  RawQueueHandle,
  RawRemoteBindingsHandle,
  RawRemoteStorageHandle,
  RawSandboxHandle,
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
  const put = vi.fn<RawRemoteStorageHandle["put"]>(async () => ({}))
  const storage: RawRemoteStorageHandle = {
    get: async path => ({
      data: Buffer.from(path),
      meta: { location: path, size: path.length, lastModified: "" },
      attributes: { metadata: {} },
    }),
    put,
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
  const key: RawKeyHandle = {
    encrypt: async (plaintext, context) =>
      Buffer.concat([plaintext, Buffer.from(context?.tenant ?? "")]),
    decrypt: async ciphertext => ciphertext,
  }
  const resolveKey = vi.fn<(name: string) => Promise<RawKeyHandle>>(async () => key)
  const session = (sessionId: string | null | undefined) => ({
    sessionId: sessionId ?? "generated",
    state: "running",
    generation: 1,
  })
  const terminate = vi.fn<RawSandboxHandle["terminate"]>(async () => {})
  const sandbox: RawSandboxHandle = {
    capabilities: () => ["files", "reconnect"],
    create: async sessionId => session(sessionId),
    get: async () => null,
    getOrCreate: async sessionId => session(sessionId),
    list: async () => [],
    runCommand: async () => {
      throw new Error("unused")
    },
    readFile: async (_sessionId, path) => Buffer.from(path),
    writeFile: async () => {},
    mkdir: async () => {},
    suspend: async () => {},
    resume: async () => {},
    terminate,
  }
  const resolveSandbox = vi.fn<(name: string) => Promise<RawSandboxHandle>>(async () => sandbox)
  const resolveAi = vi.fn<RawRemoteBindingsHandle["ai"]>(async () => ({
    resourceId: "models",
    bindingJson: JSON.stringify({ service: "bedrock", region: "us-east-1" }),
    clientConfigJson: JSON.stringify({
      platform: "aws",
      accountId: "123456789012",
      region: "us-east-1",
      credentials: { type: "sessionCredentials" },
    }),
    expiresAt: "2026-08-05T08:00:00Z",
  }))

  class FakeBindingsHandle implements RawBindingsHandle {
    key = resolveKey

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

    async postgres(): Promise<RawPostgresHandle> {
      throw new Error("unused")
    }

    async sandbox(): Promise<never> {
      throw new Error("unused")
    }
  }

  class FakeRemoteBindingsHandle implements RawRemoteBindingsHandle {
    static forCustomer: (
      project: string,
      externalId: string,
      token: string,
      apiBaseUrl?: string,
    ) => Promise<RawRemoteBindingsHandle>

    static forDeployment: (
      deploymentId: string,
      token: string,
      apiBaseUrl?: string,
    ) => Promise<RawRemoteBindingsHandle>

    storage = resolveStorage

    key = resolveKey

    sandbox = resolveSandbox

    ai = resolveAi
  }

  const forRemoteDeployment = vi.fn<
    (deploymentId: string, token: string, apiBaseUrl?: string) => Promise<RawRemoteBindingsHandle>
  >(async () => new FakeRemoteBindingsHandle())
  const forRemoteCustomer = vi.fn<
    (
      project: string,
      externalId: string,
      token: string,
      apiBaseUrl?: string,
    ) => Promise<RawRemoteBindingsHandle>
  >(async () => new FakeRemoteBindingsHandle())
  FakeRemoteBindingsHandle.forCustomer = forRemoteCustomer
  FakeRemoteBindingsHandle.forDeployment = forRemoteDeployment

  return {
    addon: {
      BindingsHandle: FakeBindingsHandle,
      RemoteBindingsHandle: FakeRemoteBindingsHandle,
      version: () => "test",
    },
    forRemoteCustomer,
    forRemoteDeployment,
    resolveStorage,
    head,
    put,
    resolveKey,
    resolveSandbox,
    terminate,
    resolveAi,
  }
}

beforeEach(() => {
  loadAddon.mockReset()
})

describe("Bindings.forRemoteCustomer", () => {
  it("forwards the Project, external ID, token, and API base URL", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)

    const bindings = await Bindings.forRemoteCustomer({
      project: "customer-files",
      externalId: "customer_123",
      token: "token_123",
      apiBaseUrl: "https://api.example.com",
    })

    expect(fixture.forRemoteCustomer).toHaveBeenCalledWith(
      "customer-files",
      "customer_123",
      "token_123",
      "https://api.example.com",
    )
    expect(bindings.storage("storage")).toBeDefined()
  })
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
    expect("key" in bindings).toBe(true)
    expect(Object.keys(storage).sort()).toEqual(["delete", "get", "head", "list", "put"])
  })

  it("resolves a typed remote Key and forwards bytes and context", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })

    const ciphertext = await bindings
      .key("customer-key")
      .encrypt(Buffer.from("root"), { context: { tenant: "acme" } })

    expect(ciphertext.toString()).toBe("rootacme")
    expect(fixture.resolveKey).toHaveBeenCalledOnce()
    expect(fixture.resolveKey).toHaveBeenCalledWith("customer-key")
  })

  it("resolves the deployment-level AI lease without a resource name", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })

    const lease = await bindings.ai()

    expect(fixture.resolveAi).toHaveBeenCalledWith()
    expect(lease.resourceId).toBe("models")
    expect(lease.binding).toEqual({ service: "bedrock", region: "us-east-1" })
    expect(lease.clientConfig.platform).toBe("aws")
    expect(lease.expiresAt).toEqual(new Date("2026-08-05T08:00:00Z"))
  })

  it("mirrors the in-cloud Sandbox surface and resolves each name lazily once", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })

    const agent = bindings.sandbox("agent")
    const build = bindings.sandbox("build")

    expect(fixture.resolveSandbox).not.toHaveBeenCalled()
    expect(bindings.sandbox("agent")).toBe(agent)
    // A remote Sandbox is the in-cloud one, so a method missing here is a caller writing
    // against a surface the hosted path silently does not have.
    expect(Object.keys(agent).sort()).toEqual(
      [
        "capabilities",
        "create",
        "get",
        "getOrCreate",
        "list",
        "runCommand",
        "readFile",
        "writeFiles",
        "mkdir",
        "suspend",
        "resume",
        "terminate",
      ].sort(),
    )

    await expect(agent.capabilities()).resolves.toEqual(["files", "reconnect"])
    await agent.terminate("session_1")
    await build.capabilities()

    expect(fixture.forRemoteDeployment).toHaveBeenCalledOnce()
    expect(fixture.resolveSandbox.mock.calls).toEqual([["agent"], ["build"]])
    expect(fixture.terminate).toHaveBeenCalledWith("session_1")
  })

  it("unwraps napi errors from Sandbox resolution and operations", async () => {
    const fixture = fakeRemoteAddon()
    fixture.resolveSandbox.mockRejectedValueOnce(
      new Error(
        JSON.stringify({
          code: "REMOTE_BINDING_DENIED",
          message: "Remote binding access denied",
          retryable: false,
        }),
      ),
    )
    loadAddon.mockReturnValue(fixture.addon)
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })

    await expect(bindings.sandbox("agent").capabilities()).rejects.toMatchObject({
      code: "REMOTE_BINDING_DENIED",
      message: "Remote binding access denied",
    })

    fixture.terminate.mockRejectedValueOnce(new Error("native transport failed"))
    const operation = bindings.sandbox("build").terminate("session_1")

    await expect(operation).rejects.toBeInstanceOf(AlienError)
    await expect(operation).rejects.toMatchObject({
      code: "BINDINGS_ERROR",
      message: "native transport failed",
    })
  })

  it("reuses one native bindings handle and resolves each Storage handle lazily once", async () => {
    const fixture = fakeRemoteAddon()
    fixture.head.mockResolvedValue({
      meta: {
        location: "archive/a.txt",
        size: 1,
        lastModified: "2026-01-01T00:00:00Z",
      },
      attributes: { metadata: {} },
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

  it("forwards object attributes through remote Storage puts", async () => {
    const fixture = fakeRemoteAddon()
    loadAddon.mockReturnValue(fixture.addon)
    const bindings = await Bindings.forRemoteDeployment({
      deploymentId: "dep_123",
      token: "token_123",
    })
    const options = {
      attributes: {
        contentType: "application/json",
        metadata: { schema: "event-v1" },
      },
    }

    await bindings.storage("archive").put("events/1.json", Buffer.from("{}"), options)

    expect(fixture.put).toHaveBeenCalledWith("events/1.json", Buffer.from("{}"), options)
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
