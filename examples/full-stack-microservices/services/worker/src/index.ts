import { storage } from "@alienplatform/sdk"
import Redis from "ioredis"
import pg from "pg"

const required = (name: string): string => {
  const value = process.env[name]
  if (!value) {
    throw new Error(`Missing required environment variable ${name}`)
  }
  return value
}

const db = new pg.Pool({ connectionString: required("DATABASE_URL") })
const redis = new Redis(required("REDIS_URL"), { maxRetriesPerRequest: null })
const files = await storage(process.env.FILES_BUCKET ?? "files")

console.log("worker waiting for Redis jobs")

while (true) {
  const item = await redis.brpop("work:issues", 0)
  if (!item) {
    continue
  }

  const payload = JSON.parse(item[1]) as { issueId: string }
  const issue = await db.query("select id, title, body from issues where id = $1", [
    payload.issueId,
  ])
  if (issue.rowCount === 0) {
    await redis.hset(`issue:${payload.issueId}:job`, {
      status: "missing",
      processedAt: new Date().toISOString(),
    })
    continue
  }

  const row = issue.rows[0]
  const artifactKey = `artifacts/${row.id}/summary.txt`
  await files.put(
    artifactKey,
    `Issue: ${row.title}\n\n${row.body}\n\nProcessed: ${new Date().toISOString()}\n`,
    { contentType: "text/plain; charset=utf-8" },
  )
  await redis.hset(`issue:${row.id}:job`, {
    status: "processed",
    artifactKey,
    processedAt: new Date().toISOString(),
  })
  await db.query("update issues set status = $1, updated_at = now() where id = $2", [
    "processed",
    row.id,
  ])
}
