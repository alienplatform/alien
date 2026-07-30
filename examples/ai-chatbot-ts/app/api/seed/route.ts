import { postgres } from "@alienplatform/sdk"
import { Client } from "pg"

// Postgres is private (same-stack only), so seeding runs inside the deployed app:
// `curl -X POST https://<app-url>/api/seed` drops and recreates the demo tables.
export async function POST() {
  const conn = await postgres("db").connection()
  // Field style, not conn.connectionString — see the chat route's pool for the sslmode footgun.
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
    await client.query("drop table if exists orders; drop table if exists customers;")
    await client.query(`
      create table customers (
        id serial primary key, name text, plan text, country text, mrr_usd int
      );
      create table orders (
        id serial primary key, customer_id int references customers(id),
        amount_usd int, status text, created date
      );
    `)
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
    const summary = await client.query(
      "select count(*)::int as customers, sum(mrr_usd)::int as total_mrr from customers",
    )
    return Response.json({ seeded: true, ...summary.rows[0] })
  } finally {
    await client.end()
  }
}
