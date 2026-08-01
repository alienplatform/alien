import { postgres } from "@alienplatform/sdk"
import { Client } from "pg"

// The app is publicly reachable, so it exposes no endpoint that writes to the
// database. Seeding only what is missing keeps a repeat call harmless.
const SCHEMA = `
  create table if not exists customers (
    id serial primary key, name text, plan text, country text, mrr_usd int
  );
  create table if not exists orders (
    id serial primary key, customer_id int references customers(id),
    amount_usd int, status text, created date
  );
`

const CUSTOMERS = `
  insert into customers (name, plan, country, mrr_usd) values
    ('Acme Corp','enterprise','US',4200),
    ('Globex','enterprise','DE',3800),
    ('Initech','pro','US',900),
    ('Umbrella','enterprise','UK',5100),
    ('Hooli','pro','IL',1200),
    ('Stark Industries','enterprise','US',6400),
    ('Wayne Enterprises','pro','US',1500),
    ('Soylent','starter','FR',150);
`

const ORDERS = `
  insert into orders (customer_id, amount_usd, status, created) values
    (1,1200,'paid','2026-05-02'),(1,800,'paid','2026-06-01'),
    (2,3800,'paid','2026-06-03'),(4,5100,'paid','2026-06-05'),
    (6,6400,'paid','2026-06-06'),(3,900,'refunded','2026-05-20'),
    (5,1200,'paid','2026-06-10'),(7,1500,'pending','2026-06-12'),
    (8,150,'paid','2026-06-14'),(6,2000,'paid','2026-06-20');
`

let seeded: Promise<void> | undefined

/** Create the demo tables and fill them once per container. */
export function ensureSeeded(): Promise<void> {
  if (!seeded) {
    seeded = run().catch(err => {
      // Don't cache a failure; the next request retries.
      seeded = undefined
      throw err
    })
  }
  return seeded
}

async function run(): Promise<void> {
  const conn = await postgres("db").connection()
  // Its own write connection: the query pool the model's tool uses is read-only.
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
    await client.query(SCHEMA)
    const { rows } = await client.query("select count(*)::int as count from customers")
    if (rows[0].count > 0) return
    await client.query(CUSTOMERS)
    await client.query(ORDERS)
  } finally {
    await client.end()
  }
}
