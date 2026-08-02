import { postgres } from "@alienplatform/sdk"
import { Pool, type QueryResult } from "pg"
import { ensureSeeded, forgetSeeded } from "./seed"

const UNDEFINED_TABLE = "42P01"

let pool: Promise<Pool> | undefined

export async function query(text: string, values: unknown[] = []): Promise<QueryResult> {
  const run = async () => (await queryPool()).query(text, values)
  try {
    return await run()
  } catch (err) {
    if ((err as { code?: string }).code !== UNDEFINED_TABLE) throw err
    forgetSeeded()
    await ensureSeeded()
    return run()
  }
}

/** The read-only pool every query in the app goes through. */
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
