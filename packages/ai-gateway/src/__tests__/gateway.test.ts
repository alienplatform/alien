import { AlienError } from "@alienplatform/core"
import { describe, expect, it, vi } from "vitest"

import { createGateway } from "../gateway.js"
import type { NativeAddon, RawAiGatewayHandle } from "../loader.js"

const handle = { url: "http://127.0.0.1:41999" } as unknown as RawAiGatewayHandle

function addonThat(startAiGateway: NativeAddon["startAiGateway"]): NativeAddon {
  return { startAiGateway, version: () => "1.11.2" } as unknown as NativeAddon
}

describe("createGateway", () => {
  it("starts the addon once and reuses the resolved handle", async () => {
    const start = vi.fn().mockResolvedValue(handle)
    const gateway = createGateway(() => addonThat(start))

    expect(await gateway.startAiGateway()).toBe(handle)
    expect(await gateway.startAiGateway()).toBe(handle)
    expect(start).toHaveBeenCalledTimes(1)
  })

  it("retries after a transient failure instead of caching the rejection", async () => {
    const start = vi
      .fn()
      .mockRejectedValueOnce(new Error("credential mint timed out"))
      .mockResolvedValue(handle)
    const gateway = createGateway(() => addonThat(start))

    await expect(gateway.startAiGateway()).rejects.toThrow(AlienError)
    // A cached rejection would leave the gateway permanently dead for this process.
    expect(await gateway.startAiGateway()).toBe(handle)
    expect(start).toHaveBeenCalledTimes(2)
  })

  it("rejects rather than throwing synchronously when the addon fails to load", async () => {
    const gateway = createGateway(() => {
      throw new Error("Cannot load the native addon for 'darwin-arm64'")
    })
    // A synchronous throw would escape a caller's `.catch()`.
    await expect(gateway.startAiGateway()).rejects.toThrow(AlienError)
  })

  it("decodes the addon's error envelope, preserving code and retryable", async () => {
    const envelope = JSON.stringify({
      code: "GATEWAY_AMBIENT_CREDENTIAL_UNAVAILABLE",
      message: "Could not obtain the workload's ambient cloud credential: metadata timeout",
      retryable: true,
    })
    const gateway = createGateway(() => addonThat(vi.fn().mockRejectedValue(new Error(envelope))))

    await expect(gateway.startAiGateway()).rejects.toMatchObject({
      code: "GATEWAY_AMBIENT_CREDENTIAL_UNAVAILABLE",
      retryable: true,
    })
  })
})
