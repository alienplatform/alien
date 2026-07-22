import { EventEmitter } from "node:events"
import { AlienError } from "@alienplatform/core"
import { afterEach, describe, expect, it, vi } from "vitest"

import { createGateway } from "../gateway.js"

// `vi.hoisted` so the (hoisted) `vi.mock` factory can reference the mock.
const { spawnMock } = vi.hoisted(() => ({ spawnMock: vi.fn() }))
vi.mock("node:child_process", () => ({ spawn: (...args: unknown[]) => spawnMock(...args) }))

/** A minimal stand-in for a spawned ChildProcess the gateway wrapper drives. */
function fakeChild() {
  const mkStream = () => {
    const s = new EventEmitter() as EventEmitter & { resume: () => void }
    s.resume = () => {}
    return s
  }
  const child = new EventEmitter() as EventEmitter & {
    stdout: EventEmitter
    stderr: EventEmitter
    kill: () => void
    unref: () => void
  }
  child.stdout = mkStream()
  child.stderr = mkStream()
  child.kill = vi.fn()
  child.unref = vi.fn()
  return child
}

const READY = '{"aiGatewayUrl":"http://127.0.0.1:41999"}\n'

afterEach(() => {
  spawnMock.mockReset()
  vi.unstubAllEnvs()
})

describe("createGateway", () => {
  it("spawns the binary once, reads the printed URL, and reuses the handle", async () => {
    spawnMock.mockImplementation(() => {
      const child = fakeChild()
      // Emit after the wrapper has attached its listeners (next microtask).
      queueMicrotask(() => child.stdout.emit("data", Buffer.from(READY)))
      return child
    })
    const gateway = createGateway(async () => "/opt/alien-ai-gateway")

    expect(await gateway.startAiGateway()).toEqual({ url: "http://127.0.0.1:41999" })
    expect(await gateway.startAiGateway()).toEqual({ url: "http://127.0.0.1:41999" })
    expect(spawnMock).toHaveBeenCalledTimes(1)
    expect(spawnMock).toHaveBeenCalledWith("/opt/alien-ai-gateway", ["--gateway-serve"], {
      stdio: ["ignore", "pipe", "pipe"],
    })
  })

  it("uses ALIEN_AI_GATEWAY_URL without spawning when a launcher already started the gateway", async () => {
    vi.stubEnv("ALIEN_AI_GATEWAY_URL", "http://127.0.0.1:9008")
    const gateway = createGateway(async () => "/opt/alien-ai-gateway")

    expect(await gateway.startAiGateway()).toEqual({ url: "http://127.0.0.1:9008" })
    expect(spawnMock).not.toHaveBeenCalled()
  })

  it("retries after a transient startup failure instead of caching the rejection", async () => {
    spawnMock
      .mockImplementationOnce(() => {
        const child = fakeChild()
        // Exit before printing a URL, with a reason on stderr.
        queueMicrotask(() => {
          child.stderr.emit("data", Buffer.from("ambient credential not ready"))
          child.emit("exit", 1, null)
        })
        return child
      })
      .mockImplementationOnce(() => {
        const child = fakeChild()
        queueMicrotask(() => child.stdout.emit("data", Buffer.from(READY)))
        return child
      })
    const gateway = createGateway(async () => "/opt/alien-ai-gateway")

    await expect(gateway.startAiGateway()).rejects.toThrow(AlienError)
    // A cached rejection would leave the gateway permanently dead for this process.
    expect(await gateway.startAiGateway()).toEqual({ url: "http://127.0.0.1:41999" })
    expect(spawnMock).toHaveBeenCalledTimes(2)
  })

  it("surfaces the child's stderr as the startup failure reason", async () => {
    spawnMock.mockImplementation(() => {
      const child = fakeChild()
      queueMicrotask(() => {
        child.stderr.emit("data", Buffer.from("bind: address already in use"))
        child.emit("exit", 1, null)
      })
      return child
    })
    const gateway = createGateway(async () => "/opt/alien-ai-gateway")

    await expect(gateway.startAiGateway()).rejects.toThrow(/address already in use/)
  })

  it("rejects rather than throwing synchronously when the binary cannot be resolved", async () => {
    const gateway = createGateway(async () => {
      throw new Error("no alien-ai-gateway binary for this platform")
    })
    // A synchronous throw would escape a caller's `.catch()`.
    await expect(gateway.startAiGateway()).rejects.toThrow()
    expect(spawnMock).not.toHaveBeenCalled()
  })
})
