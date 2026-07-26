/**
 * Postgres behavioral tests through the REAL napi addon against the inline-password
 * providers (`crates/alien-bindings/src/providers/postgres/local.rs`). No mocks: the
 * binding JSON goes into the environment and the Rust resolver produces the connection.
 *
 * The three cloud backends (Aurora / Cloud SQL / Flexible Server) resolve their password
 * from a cloud secret store, which needs real cloud credentials — they are covered by
 * unit tests with a faked secret client in each provider module, and end to end only
 * against a deployed stack (see the PR notes).
 */

import { afterAll, describe, expect, it } from "vitest"
import { AlienError, postgres } from "../src/index.js"
import {
  POSTGRES_FIXTURE,
  cleanupTempDirs,
  externalPostgresBindingEnv,
  localPostgresBindingEnv,
} from "./helpers/local-binding-env.js"

const isBun = process.env.BUN_EXPECTED === "1"

function localName(): string {
  const name = isBun ? "bun-postgres-local" : "postgres-local"
  if (!isBun) localPostgresBindingEnv(name)
  return name
}

function externalName(): string {
  const name = isBun ? "bun-postgres-external" : "postgres-external"
  if (!isBun) externalPostgresBindingEnv(name)
  return name
}

describe("postgres (inline-password providers)", () => {
  afterAll(() => {
    cleanupTempDirs()
  })

  // The exact URL is the contract: the password's RFC 3986 sub-delims (! * ' ( )) and
  // its `@` / `/` must be percent-encoded, or the URL would be unparseable or point at
  // the wrong host. Pinned as a literal so any change to the encoding or to the shared
  // fixture fails here rather than silently producing a different connection.
  it("resolves a local binding to a sslmode=disable connection with an encoded password", async () => {
    const connection = await postgres(localName()).connection()

    expect(connection.connectionString).toBe(
      "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@db.internal:5432/app?sslmode=disable",
    )
    expect(connection.sslmode).toBe("disable")
    expect(connection.ssl).toBe(false)
  })

  it("resolves an external binding to a sslmode=prefer connection", async () => {
    const connection = await postgres(externalName()).connection()

    expect(connection.connectionString).toBe(
      "postgres://alien:a%21b%2Ac%27d%28e%29f%40%2F@db.internal:5432/app?sslmode=prefer",
    )
    expect(connection.sslmode).toBe("prefer")
    // node-postgres has no `prefer` mode, so the driver field stays plaintext while the
    // URL keeps `sslmode=prefer` for sslmode-aware consumers such as psql.
    expect(connection.ssl).toBe(false)
  })

  // The individual fields are what a driver taking separate arguments gets, so they must
  // carry the *un-encoded* values the connection string encodes.
  it("returns the un-encoded connection fields alongside the URL", async () => {
    const connection = await postgres(localName()).connection()

    expect(connection.host).toBe(POSTGRES_FIXTURE.host)
    expect(connection.port).toBe(POSTGRES_FIXTURE.port)
    expect(connection.database).toBe(POSTGRES_FIXTURE.database)
    expect(connection.username).toBe(POSTGRES_FIXTURE.username)
    expect(connection.password).toBe(POSTGRES_FIXTURE.password)
  })

  it("reports an unconfigured binding rather than resolving an empty connection", async () => {
    const error = await postgres("postgres-not-configured")
      .connection()
      .catch((e: unknown) => e)

    expect(error).toBeInstanceOf(AlienError)
    expect((error as AlienError).code).toBe("BINDING_NOT_CONFIGURED")
    expect((error as AlienError).message).toContain("ALIEN_POSTGRES_NOT_CONFIGURED_BINDING")
  })
})
