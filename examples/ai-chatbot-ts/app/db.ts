import { postgres } from "@alienplatform/sdk"
import { Pool, type QueryResult } from "pg"
import { ensureSeeded, forgetSeeded } from "./seed"

const UNDEFINED_TABLE = "42P01"

// The binding reads the password from the cloud secret store at runtime with the
// workload's own identity — it is never in the environment.
let pool: Promise<Pool> | undefined

export async function query(sql: string): Promise<QueryResult> {
  try {
    return await readOnly(sql)
  } catch (err) {
    if ((err as { code?: string }).code !== UNDEFINED_TABLE) throw err
    forgetSeeded()
    await ensureSeeded()
    return readOnly(sql)
  }
}

// `begin read only` and not just the session default: the model's statement runs
// on a pooled connection, and a session setting is reversible from inside the very
// SQL being bounded (`select set_config('default_transaction_read_only','off')`).
// A read-only transaction cannot be reopened for writing, so the bound holds.
async function readOnly(sql: string): Promise<QueryResult> {
  const pool = await queryPool()
  const client = await pool.connect()
  try {
    await client.query("begin read only")
    const result = await client.query(sql)
    await client.query("commit")
    return result
  } catch (err) {
    await client.query("rollback").catch(() => {})
    throw err
  } finally {
    client.release()
  }
}

/** The pool every read goes through, including the SQL the model writes. */
export function queryPool(): Promise<Pool> {
  if (!pool) {
    pool = (async () => {
      const conn = await postgres("db").connection()
      // Field style + conn.ssl, NOT conn.connectionString: node-postgres parses the
      // URL's sslmode and overrides ssl, which breaks the managed-cloud cert path.
      return new Pool({
        host: conn.host,
        port: conn.port,
        database: conn.database,
        user: conn.username,
        password: conn.password,
        ssl: conn.ssl,
        // The timeout stops a `pg_sleep` or runaway scan from pinning a connection;
        // writes are stopped per transaction in `readOnly`, not by this default.
        options: "-c default_transaction_read_only=on -c statement_timeout=10000",
      })
    })().catch(err => {
      // Don't cache a failed resolution; let the next request retry.
      pool = undefined
      throw err
    })
  }
  return pool
}
