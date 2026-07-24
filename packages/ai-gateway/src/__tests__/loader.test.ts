import { existsSync, readFileSync } from "node:fs"
import { afterEach, describe, expect, it, vi } from "vitest"

import {
  platformTriple,
  registerEmbeddedBinary,
  resetGatewayBinaryCacheForTests,
  resolveGatewayBinary,
} from "../loader.js"

afterEach(() => {
  resetGatewayBinaryCacheForTests()
  vi.unstubAllEnvs()
  ;(globalThis as { Bun?: unknown }).Bun = undefined
})

describe("platformTriple", () => {
  it("maps known platforms, including musl (the binary supports Alpine)", () => {
    expect(platformTriple("darwin", "arm64")).toBe("darwin-arm64")
    expect(platformTriple("darwin", "x64")).toBe("darwin-x64")
    expect(platformTriple("linux", "x64", "gnu")).toBe("linux-x64-gnu")
    // The napi addon rejected musl; the standalone binary supports it.
    expect(platformTriple("linux", "arm64", "musl")).toBe("linux-arm64-musl")
  })

  it("throws on an unsupported platform", () => {
    expect(() => platformTriple("sunos" as NodeJS.Platform, "x64")).toThrow()
  })
})

describe("resolveGatewayBinary", () => {
  it("uses ALIEN_AI_GATEWAY_BINARY_PATH when set", async () => {
    vi.stubEnv("ALIEN_AI_GATEWAY_BINARY_PATH", "/custom/alien-ai-gateway")
    expect(await resolveGatewayBinary()).toBe("/custom/alien-ai-gateway")
  })

  it("extracts a registered embedded binary to a runnable file (compiled Worker path)", async () => {
    const contents = "#!/bin/sh\nexit 0\n"
    const bytes = new TextEncoder().encode(contents)
    // A `bun build --compile` binary registers its embedded copy; the loader must
    // prefer it and extract it to a real, executable file (the embedded path is
    // virtual). Stub the Bun runtime the extractor uses.
    ;(globalThis as { Bun?: unknown }).Bun = {
      file: () => ({ arrayBuffer: async () => bytes.buffer }),
    }
    registerEmbeddedBinary("/$bunfs/root/alien-ai-gateway.bin")

    const extracted = await resolveGatewayBinary()
    expect(extracted).toMatch(/alien-ai-gateway$/)
    expect(existsSync(extracted)).toBe(true)
    expect(readFileSync(extracted, "utf8")).toBe(contents)
  })
})
