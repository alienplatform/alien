import { postgres } from "@alienplatform/sdk"
import { Pool, type QueryResult } from "pg"
import { ensureSeeded, forgetSeeded } from "./seed"

const UNDEFINED_TABLE = "42P01"

// The binding reads the password from the cloud secret store at runtime with the
// workload's own identity — it is never in the environment.
let pool: Promise<Pool> | undefined

export async function query(sql: string): Promise<QueryResult> {
  const pool = await queryPool()
  try {
    return await pool.query(sql)
  } catch (err) {
    if ((err as { code?: string }).code !== UNDEFINED_TABLE) throw err
    forgetSeeded()
    await ensureSeeded()
    return pool.query(sql)
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
        // The model writes the SQL, so the session bounds it rather than the parser:
        // read-only stops writes, and the timeout stops a `pg_sleep` or runaway scan
        // from pinning a pool connection.
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
