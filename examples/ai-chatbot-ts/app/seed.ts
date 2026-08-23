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
  insert into customers (id, name, plan, country, mrr_usd) values
    (1,'Acme Corp','enterprise','US',4200),
    (2,'Globex','enterprise','DE',3800),
    (3,'Initech','pro','US',900),
    (4,'Umbrella','enterprise','UK',5100),
    (5,'Hooli','pro','IL',1200),
    (6,'Stark Industries','enterprise','US',6400),
    (7,'Wayne Enterprises','pro','US',1500),
    (8,'Soylent','starter','FR',150)
  on conflict (id) do update set
    name = excluded.name,
    plan = excluded.plan,
    country = excluded.country,
    mrr_usd = excluded.mrr_usd;
`

const ORDERS = `
  insert into orders (id, customer_id, amount_usd, status, created) values
    (1,1,1200,'paid','2026-05-02'),(2,1,800,'paid','2026-06-01'),
    (3,2,3800,'paid','2026-06-03'),(4,4,5100,'paid','2026-06-05'),
    (5,6,6400,'paid','2026-06-06'),(6,3,900,'refunded','2026-05-20'),
    (7,5,1200,'paid','2026-06-10'),(8,7,1500,'pending','2026-06-12'),
    (9,8,150,'paid','2026-06-14'),(10,6,2000,'paid','2026-06-20')
  on conflict (id) do update set
    customer_id = excluded.customer_id,
    amount_usd = excluded.amount_usd,
    status = excluded.status,
    created = excluded.created;
`

const SEED_LOCK = 4212025

let seeded: Promise<void> | undefined

/** Create the demo tables and fill them, at most once per container. */
export function ensureSeeded(): Promise<void> {
  if (!seeded) {
    seeded = run().catch(err => {
      seeded = undefined
      throw err
    })
  }
  return seeded
}

export function forgetSeeded(): void {
  seeded = undefined
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
    // A container that dies holding the advisory lock would otherwise park every
    // other container's seed on `pg_advisory_lock` forever.
    options: "-c statement_timeout=30000",
  })
  await client.connect()
  try {
    await client.query("select pg_advisory_lock($1)", [SEED_LOCK])
    await client.query("begin")
    try {
      await client.query(SCHEMA)
      // Stable IDs plus upserts repair an interrupted or partially completed seed.
      await client.query(CUSTOMERS)
      await client.query(ORDERS)
      await client.query(
        "select setval(pg_get_serial_sequence('customers', 'id'), greatest(max(id), 1)) from customers",
      )
      await client.query(
        "select setval(pg_get_serial_sequence('orders', 'id'), greatest(max(id), 1)) from orders",
      )
      await client.query("commit")
    } catch (err) {
      await client.query("rollback")
      throw err
    }
  } finally {
    await client.end()
  }
}
