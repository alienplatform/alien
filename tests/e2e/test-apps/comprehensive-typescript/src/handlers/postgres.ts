import { AlienError } from "@alienplatform/core"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

// Bun's built-in Postgres client. The worker runs on Bun (`bun build --compile`), so `Bun.SQL` is a
// runtime global — used via the global rather than `import { SQL } from "bun"` so the bare specifier
// never has to be bundled. Declared locally because the package carries no `@types/bun`; the build
// does not type-check, so this is for editor hygiene only.
type BunSqlClient = {
  (strings: TemplateStringsArray, ...values: unknown[]): Promise<Array<Record<string, unknown>>>
  end(): Promise<void>
}
declare const Bun: { SQL: new (connectionString: string) => BunSqlClient }

// Postgres connection resolution, inlined here from the (now removed)
// `@alienplatform/sdk` `getPostgresConnection`. The SDK facade is Worker
// handler APIs + binding factories only; connection-only Postgres resolution
// lives with its sole consumer. This e2e app only deploys local Postgres
// (plaintext, `sslmode=disable`), so only the non-cloud variants are inlined;
// cloud secret resolution (Aurora/Cloud SQL/Flexible Server) is not exercised
// here and is intentionally rejected rather than pulling in cloud secret SDKs.

/** Matches the Rust `binding_env_var_name`: `ALIEN_<NAME>_BINDING`, hyphens to underscores. */
function bindingEnvVarName(bindingName: string): string {
  return `ALIEN_${bindingName.replace(/-/g, "_").toUpperCase()}_BINDING`
}

/**
 * Percent-encode a userinfo/path component to the RFC 3986 unreserved set,
 * matching the Rust resolver.
 */
function encodeUserinfo(value: string): string {
  return encodeURIComponent(value).replace(
    /[!'()*]/g,
    character =>
      `%${character.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`,
  )
}

function connectionString(
  host: string,
  params: { port: number; database: string; username: string },
  password: string,
  sslmode: "disable" | "prefer",
): string {
  const user = encodeUserinfo(params.username)
  const pass = encodeUserinfo(password)
  return `postgres://${user}:${pass}@${host}:${params.port}/${encodeUserinfo(params.database)}?sslmode=${sslmode}`
}

/** Resolve the connection string for a linked local/external Postgres database. */
function resolveConnectionString(bindingName: string): string {
  const raw = process.env[bindingEnvVarName(bindingName)]
  if (!raw) {
    throw new AlienError({
      code: "POSTGRES_BINDING_NOT_FOUND",
      message: `Postgres binding '${bindingName}' is not configured`,
      retryable: false,
      internal: false,
      httpStatusCode: 404,
      context: { bindingName },
    })
  }
  const binding = JSON.parse(raw) as {
    service: string
    host: string
    port: number
    database: string
    username: string
    password?: string
  }
  const params = { port: binding.port, database: binding.database, username: binding.username }
  switch (binding.service) {
    case "local-postgres":
      return connectionString(binding.host, params, binding.password ?? "", "disable")
    case "external":
      return connectionString(binding.host, params, binding.password ?? "", "prefer")
    default:
      throw new AlienError({
        code: "POSTGRES_BINDING_UNSUPPORTED",
        message: `Postgres backend '${binding.service}' is not exercised by the e2e test app`,
        retryable: false,
        internal: false,
        httpStatusCode: 400,
        context: { service: binding.service },
      })
  }
}

const app = new Hono()

// Exercise a Postgres binding end to end: resolve the connection, open a real driver connection, and
// run a query. The binding is connection-only by design (no gRPC surface), so proving the resource
// works means actually speaking the wire protocol against it.
app.post("/postgres-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const url = resolveConnectionString(bindingName)
    // Local Postgres is plaintext (sslmode=disable); the connection string carries the credentials.
    const sql = new Bun.SQL(url)
    try {
      const rows = await sql`SELECT 1 AS one`
      const one = rows[0]?.one
      if (one !== 1) {
        return c.json(
          { success: false, error: `unexpected query result: ${JSON.stringify(rows[0])}` },
          500,
        )
      }
    } finally {
      await sql.end()
    }
    return c.json({ success: true, bindingName })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "postgres-test")
    return c.json({ success: false, error: alienError.message, code: alienError.code }, 500)
  }
})

export default app
