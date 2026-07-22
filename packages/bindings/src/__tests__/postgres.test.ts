import { afterEach, describe, expect, it, vi } from "vitest"

import { encodeUserinfo, getPostgresConnection } from "../postgres.js"

afterEach(() => {
  vi.unstubAllEnvs()
})

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

describe("getPostgresConnection", () => {
  it("resolves a local-postgres binding with sslmode=disable and no TLS", async () => {
    vi.stubEnv(
      "ALIEN_DB_BINDING",
      JSON.stringify({
        service: "local-postgres",
        host: "127.0.0.1",
        port: 5433,
        database: "app",
        username: "app_user",
        password: "p@ss/word",
      }),
    )
    const conn = await getPostgresConnection("db")
    expect(conn.ssl).toBe(false)
    expect(conn.host).toBe("127.0.0.1")
    expect(conn.password).toBe("p@ss/word")
    expect(conn.connectionString).toBe(
      "postgres://app_user:p%40ss%2Fword@127.0.0.1:5433/app?sslmode=disable",
    )
  })

  it("resolves an external (BYO) binding with sslmode=prefer", async () => {
    vi.stubEnv(
      "ALIEN_DB_BINDING",
      JSON.stringify({
        service: "external",
        host: "db.example.com",
        port: 5432,
        database: "app",
        username: "u",
        password: "p",
      }),
    )
    const conn = await getPostgresConnection("db")
    expect(conn.ssl).toBe(false)
    expect(conn.connectionString).toContain("sslmode=prefer")
  })

  it("throws BINDING_NOT_FOUND when the env var is missing", async () => {
    await expect(getPostgresConnection("missing")).rejects.toMatchObject({
      code: "BINDING_NOT_FOUND",
    })
  })

  it("throws INVALID_BINDING_CONFIG on malformed JSON", async () => {
    vi.stubEnv("ALIEN_DB_BINDING", "{not json")
    await expect(getPostgresConnection("db")).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
  })

  it("rejects an unexpected key (strict schema at the trust boundary)", async () => {
    vi.stubEnv(
      "ALIEN_DB_BINDING",
      JSON.stringify({
        service: "local-postgres",
        host: "127.0.0.1",
        port: 5433,
        database: "app",
        username: "u",
        password: "p",
        extraKey: "tampered",
      }),
    )
    await expect(getPostgresConnection("db")).rejects.toMatchObject({
      code: "INVALID_BINDING_CONFIG",
    })
  })
})
