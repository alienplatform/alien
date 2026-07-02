import { describe, expect, it } from "vitest"

import { encodeUserinfo } from "./postgres.js"

// The TS `encodeUserinfo` must byte-for-byte match the Rust resolver's `encode_userinfo`
// (crates/alien-bindings/src/providers/postgres/local.rs), or the two runtimes produce mismatched
// connection strings for the same binding. These are the exact vectors the Rust tests pin, so a
// drift in either encoder fails on one side.
describe("encodeUserinfo", () => {
  it("percent-encodes the RFC 3986 reserved characters like the Rust resolver", () => {
    expect(encodeUserinfo("p@ss/word")).toBe("p%40ss%2Fword")
  })

  it("percent-encodes the sub-delims encodeURIComponent leaves literal (! * ' ( ))", () => {
    expect(encodeUserinfo("a!b*c'd(e)f")).toBe("a%21b%2Ac%27d%28e%29f")
  })
})
