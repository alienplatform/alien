import { describe, expect, it } from "vitest"
import { isStaleLocalAddon } from "../loader.js"

describe("isStaleLocalAddon", () => {
  it("is not stale when the addon version matches the package version", () => {
    expect(isStaleLocalAddon("1.10.7", "1.10.7")).toBe(false)
  })

  it("is stale when the addon version differs from the package version", () => {
    expect(isStaleLocalAddon("1.10.6", "1.10.7")).toBe(true)
  })
})
