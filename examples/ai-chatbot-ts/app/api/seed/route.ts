import { timingSafeEqual } from "node:crypto"
import { getPostgresConnection } from "@alienplatform/sdk"
import { Client } from "pg"

// The app's endpoint is public, so dropping the tables is gated on a secret the operator
// sets, and refuses to run when that secret is unset. Plain seeding is idempotent and needs
// no secret: it creates the tables if absent and inserts the demo rows only into an empty
// database, so a stray request can never destroy data.
function authorizedToReset(request: Request): boolean {
  const expected = process.env.SEED_TOKEN
  if (!expected) return false
  const provided = request.headers.get("x-seed-token") ?? ""
  const a = Buffer.from(provided)
  const b = Buffer.from(expected)
  // timingSafeEqual throws on a length mismatch, which would itself leak the length.
  return a.length === b.length && timingSafeEqual(a, b)
}

// Postgres is private (same-stack only), so it cannot be seeded from a laptop.
// This route runs inside the deployed app and seeds the demo data on demand:
//   curl -X POST https://<app-url>/api/seed
// To wipe and reseed:
//   curl -X POST -H "x-seed-token: $SEED_TOKEN" 'https://<app-url>/api/seed?reset=1'
export async function POST(request: Request) {
  const reset = new URL(request.url).searchParams.get("reset") === "1"
  if (reset && !authorizedToReset(request)) {
    return Response.json({ error: "unauthorized" }, { status: 401 })
  }

  const conn = await getPostgresConnection("db")
  // Field style + conn.ssl, NOT conn.connectionString (the sslmode footgun).
  const client = new Client({
    host: conn.host,
    port: conn.port,
    database: conn.database,
    user: conn.username,
    password: conn.password,
    ssl: conn.ssl,
  })
  await client.connect()
  try {
    if (reset) {
      await client.query("drop table if exists orders; drop table if exists customers;")
    }
    await client.query(`
      create table if not exists customers (
        id serial primary key, name text, plan text, country text, mrr_usd int
      );
      create table if not exists orders (
        id serial primary key, customer_id int references customers(id),
        amount_usd int, status text, created date
      );
    `)
    // Only an empty database is seeded, so a repeated request is a no-op rather than a
    // duplicate insert.
    const existing = await client.query("select count(*)::int as n from customers")
    if (existing.rows[0].n === 0) {
      await client.query(`
        insert into customers (name, plan, country, mrr_usd) values
          ('Acme Corp','enterprise','US',4200),
          ('Globex','enterprise','DE',3800),
          ('Initech','pro','US',900),
          ('Umbrella','enterprise','UK',5100),
          ('Hooli','pro','IL',1200),
          ('Stark Industries','enterprise','US',6400),
          ('Wayne Enterprises','pro','US',1500),
          ('Soylent','starter','FR',150);
      `)
      await client.query(`
        insert into orders (customer_id, amount_usd, status, created) values
          (1,1200,'paid','2026-05-02'),(1,800,'paid','2026-06-01'),
          (2,3800,'paid','2026-06-03'),(4,5100,'paid','2026-06-05'),
          (6,6400,'paid','2026-06-06'),(3,900,'refunded','2026-05-20'),
          (5,1200,'paid','2026-06-10'),(7,1500,'pending','2026-06-12'),
          (8,150,'paid','2026-06-14'),(6,2000,'paid','2026-06-20');
      `)
    }
    const summary = await client.query(
      "select count(*)::int as customers, sum(mrr_usd)::int as total_mrr from customers",
    )
    return Response.json({ seeded: true, ...summary.rows[0] })
  } finally {
    await client.end()
  }
}
