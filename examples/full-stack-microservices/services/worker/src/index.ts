import { createCommandReceiver } from "@alienplatform/commands"
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
const files = storage(process.env.FILES_BUCKET ?? "files")

// --- Command receiver (Container using @alienplatform/commands) ---
// A Container is a resident process, so it leases commands over outbound HTTPS
// (the pull model) instead of receiving pushed HTTP invocations like a Worker.
// Here the worker exposes a `reprocess` command that re-enqueues an issue for
// processing. The receiver env quintet (ALIEN_COMMANDS_URL and friends) is
// injected by the platform for command-capable resources; when it's absent
// (e.g. running the container by hand) we simply skip starting the receiver so
// the Redis job loop still runs.
if (process.env.ALIEN_COMMANDS_URL) {
  const receiver = createCommandReceiver()
  receiver.command("reprocess", async input => {
    if (typeof input !== "object" || input === null || !("issueId" in input) || typeof input.issueId !== "string") {
      throw new TypeError("issueId must be a string")
    }
    const { issueId } = input
    await redis.lpush("work:issues", JSON.stringify({ issueId, requestedAt: Date.now() }))
    return { requeued: true, issueId }
  })
  void receiver.run().catch(error => {
    console.error("command receiver stopped", error)
  })
}

console.log("worker waiting for Redis jobs")

async function runWorker() {
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
      new TextEncoder().encode(
        `Issue: ${row.title}\n\n${row.body}\n\nProcessed: ${new Date().toISOString()}\n`,
      ),
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
}

void runWorker().catch(error => {
  console.error("worker failed", error)
  process.exit(1)
})
