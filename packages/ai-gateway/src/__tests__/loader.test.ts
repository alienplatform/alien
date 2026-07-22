import { afterEach, describe, expect, it } from "vitest"
import {
  type NativeAddon,
  loadAddon,
  registerEmbeddedAddon,
  resetAddonCacheForTests,
} from "../loader.js"

describe("loadAddon embedded hatch", () => {
  afterEach(() => {
    resetAddonCacheForTests()
  })

  it("prefers a registered embedded addon over filesystem resolution", () => {
    const embedded: NativeAddon = {
      startAiGateway: async () => ({ url: "http://127.0.0.1:0" }),
      version: () => "embedded-test",
    }
    // A `bun build --compile` binary registers its embedded addon up front (via
    // the `/native` entry's installEmbeddedAddon). loadAddon must return it
    // rather than probing the filesystem/prebuild, which cannot work inside the
    // single-file binary. Registering a fake proves the embedded slot short-
    // circuits resolution.
    registerEmbeddedAddon(embedded)
    expect(loadAddon()).toBe(embedded)
  })
})
