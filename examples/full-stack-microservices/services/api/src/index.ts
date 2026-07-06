import { randomUUID } from "node:crypto"
import { storage } from "@alienplatform/sdk"
import { Hono } from "hono"
import Redis from "ioredis"
import pg from "pg"

const required = (name: string): string => {
  const value = process.env[name]
  if (!value) {
    throw new Error(`Missing required environment variable ${name}`)
  }
  return value
}

const appSecret = process.env.APP_SECRET
const databaseUrl = required("DATABASE_URL")
const redisUrl = required("REDIS_URL")
const filesBucket = process.env.FILES_BUCKET ?? "files"
const port = Number(process.env.PORT ?? "3000")
const startupTimeoutMs = Number(process.env.STARTUP_TIMEOUT_SECONDS ?? "180") * 1000

const db = new pg.Pool({ connectionString: databaseUrl })
const redis = new Redis(redisUrl, { maxRetriesPerRequest: 3 })
const files = storage(filesBucket)

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

async function waitForStartupDependency(name: string, check: () => Promise<void>) {
  const deadline = Date.now() + startupTimeoutMs
  let attempt = 0
  while (true) {
    attempt += 1
    try {
      await check()
      return
    } catch (error) {
      if (Date.now() >= deadline) {
        throw new Error(`${name} did not become ready within ${startupTimeoutMs}ms`, {
          cause: error,
        })
      }
      console.warn(`${name} is not ready yet; retrying`, { attempt, error })
      await sleep(Math.min(1000 * attempt, 5000))
    }
  }
}

async function initializeSchema() {
  await db.query(`
    create table if not exists issues (
      id text primary key,
      title text not null,
      body text not null,
      status text not null,
      created_at timestamptz not null default now(),
      updated_at timestamptz not null default now()
    )
  `)

  await db.query(`
    create table if not exists issue_files (
      id text primary key,
      issue_id text not null references issues(id) on delete cascade,
      object_key text not null,
      filename text not null,
      created_at timestamptz not null default now()
    )
  `)
}

await waitForStartupDependency("postgres", async () => {
  await db.query("select 1")
})
await initializeSchema()
await waitForStartupDependency("redis", async () => {
  await redis.ping()
})

const app = new Hono()

app.get("/health", c => c.json({ ok: true, service: "api" }))

app.get("/issues", async c => {
  const result = await db.query(
    "select id, title, body, status, created_at, updated_at from issues order by created_at desc limit 50",
  )
  return c.json({ issues: result.rows })
})

app.post("/issues", async c => {
  const body = await c.req.json<{ title?: string; body?: string }>()
  if (!body.title || !body.body) {
    return c.json({ error: "title and body are required" }, 400)
  }

  const id = randomUUID()
  const result = await db.query(
    "insert into issues (id, title, body, status) values ($1, $2, $3, $4) returning *",
    [id, body.title, body.body, "open"],
  )
  return c.json({ issue: result.rows[0] }, 201)
})

app.get("/issues/:id", async c => {
  const id = c.req.param("id")
  const issue = await db.query("select * from issues where id = $1", [id])
  if (issue.rowCount === 0) {
    return c.json({ error: "issue not found" }, 404)
  }

  const filesResult = await db.query("select * from issue_files where issue_id = $1", [id])
  const job = await redis.hgetall(`issue:${id}:job`)
  return c.json({ issue: issue.rows[0], files: filesResult.rows, job })
})

app.post("/issues/:id/files", async c => {
  const id = c.req.param("id")
  const issue = await db.query("select id from issues where id = $1", [id])
  if (issue.rowCount === 0) {
    return c.json({ error: "issue not found" }, 404)
  }

  const payload = await c.req.json<{ filename?: string; content?: string }>()
  if (!payload.filename || payload.content === undefined) {
    return c.json({ error: "filename and content are required" }, 400)
  }

  const fileId = randomUUID()
  const objectKey = `issues/${id}/${fileId}-${payload.filename}`
  await files.put(objectKey, new TextEncoder().encode(payload.content))
  const result = await db.query(
    "insert into issue_files (id, issue_id, object_key, filename) values ($1, $2, $3, $4) returning *",
    [fileId, id, objectKey, payload.filename],
  )
  return c.json({ file: result.rows[0] }, 201)
})

app.get("/files/:fileId", async c => {
  const fileId = c.req.param("fileId")
  const result = await db.query("select object_key, filename from issue_files where id = $1", [
    fileId,
  ])
  if (result.rowCount === 0) {
    return c.json({ error: "file not found" }, 404)
  }

  const object = new TextDecoder().decode(await files.get(result.rows[0].object_key))
  return c.json({ filename: result.rows[0].filename, content: object })
})

app.post("/issues/:id/process", async c => {
  const id = c.req.param("id")
  const issue = await db.query("select id from issues where id = $1", [id])
  if (issue.rowCount === 0) {
    return c.json({ error: "issue not found" }, 404)
  }

  await redis.hset(`issue:${id}:job`, {
    status: "queued",
    queuedAt: new Date().toISOString(),
  })
  await redis.lpush("work:issues", JSON.stringify({ issueId: id, requestedAt: Date.now() }))
  return c.json({ queued: true, issueId: id })
})

app.post("/internal/maintenance", async c => {
  if (appSecret && c.req.header("x-app-secret") !== appSecret) {
    return c.json({ error: "unauthorized" }, 401)
  }

  const title = `Maintenance ${new Date().toISOString()}`
  const id = randomUUID()
  await db.query("insert into issues (id, title, body, status) values ($1, $2, $3, $4)", [
    id,
    title,
    "Scheduled maintenance issue generated by the scheduler.",
    "open",
  ])
  await redis.lpush("work:issues", JSON.stringify({ issueId: id, requestedAt: Date.now() }))
  return c.json({ created: true, issueId: id })
})

if (import.meta.main) {
  Bun.serve({
    port,
    fetch: app.fetch,
  })

  console.log(`api listening on ${port}`)
}

export default app
