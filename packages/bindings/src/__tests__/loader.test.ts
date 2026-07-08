import { describe, expect, it } from "vitest"
import { platformTriple } from "../loader.js"

describe("platformTriple", () => {
  // Pins the full platform/arch/libc -> napi triple mapping table (the
  // optionalDependencies set in PACKAGE_LAYOUT.md). The existing loader suites
  // bypass this function entirely via ALIEN_BINDINGS_ADDON_PATH, which is how
  // the missing darwin-x64 branch slipped through — this test exercises
  // platformTriple directly so every combination is pinned regardless of the
  // host running the suite. libc is passed explicitly so a musl CI runner
  // cannot flip the glibc expectations.
  it.each([
    ["darwin", "arm64", "gnu", "darwin-arm64"],
    ["darwin", "x64", "gnu", "darwin-x64"],
    ["linux", "x64", "gnu", "linux-x64-gnu"],
    ["linux", "arm64", "gnu", "linux-arm64-gnu"],
  ] as const)("maps %s/%s (%s) to %s", (platform, arch, libc, triple) => {
    expect(platformTriple(platform, arch, libc)).toBe(triple)
  })

  it.each([
    ["linux", "x64"],
    ["linux", "arm64"],
  ] as const)("throws a musl-specific error on musl %s/%s", (platform, arch) => {
    // Only glibc prebuilds are published, so a musl host must fail loudly
    // naming musl rather than silently selecting a glibc triple.
    expect(() => platformTriple(platform, arch, "musl")).toThrow(
      `@alienplatform/bindings has no native addon for musl-based Linux (arch '${arch}').`,
    )
  })

  it("throws a clear error for an unsupported platform/arch pair", () => {
    expect(() => platformTriple("win32", "x64", "gnu")).toThrow(
      "@alienplatform/bindings has no native addon for platform 'win32' arch 'x64'.",
    )
  })
})
