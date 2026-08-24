import { query } from "../../db"
import { ensureSeeded } from "../../seed"

const TABLES = ["customers", "orders"] as const
const PREVIEW_ROWS = 8

/** The demo tables behind the chat, so the answers can be checked against the data. */
export async function GET() {
  await ensureSeeded()

  const tables = await Promise.all(
    TABLES.map(async name => {
      // The identifiers are this module's own constants, never request input.
      const rows = await query(`select * from ${name} order by id limit ${PREVIEW_ROWS}`)
      const total = await query(`select count(*)::int as count from ${name}`)
      return { name, rows: rows.rows, total: total.rows[0].count as number }
    }),
  )

  return Response.json({ tables })
}
