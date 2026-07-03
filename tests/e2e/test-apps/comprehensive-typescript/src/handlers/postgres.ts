import { getPostgresConnection } from "@alienplatform/sdk"
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

const app = new Hono()

// Exercise a Postgres binding end to end: resolve the connection, open a real driver connection, and
// run a query. The binding is connection-only by design (no gRPC surface), so proving the resource
// works means actually speaking the wire protocol against it.
app.post("/postgres-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  try {
    const connection = await getPostgresConnection(bindingName)
    // Local Postgres is plaintext (sslmode=disable); the connection string carries the credentials.
    const sql = new Bun.SQL(connection.connectionString)
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
