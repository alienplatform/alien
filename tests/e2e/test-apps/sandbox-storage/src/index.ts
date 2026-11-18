import { sandbox } from "@alienplatform/sdk"
import type { Context } from "hono"
import { Hono } from "hono"
import { ISOLATION, OVERSIZE, prepare, RESTORED, VERIFY, writer } from "./scripts"

// Each endpoint is one whole check: it creates the sandbox, runs a fixed program, syncs, and ends
// the sandbox before it answers. On GCP this Worker's binding is what pulls the sandbox's commits
// into the store, and Cloud Run gives it CPU only while a request is open, so nothing here leaves
// a commit for a later request to publish.

const app = new Hono()
const agents = sandbox("agents")
const SANDBOX_MS = 1_200_000
const COMMAND_MS = 600_000
const KEY = /^[A-Za-z0-9._-]{1,128}$/

app.get("/health", c => c.json({ status: "ok" }))

app.onError((error, c) =>
  c.json({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) }, 500),
)

// The endpoints are public: the caller must present the deployment's own id, which the test knows
// and a stranger reaching the URL does not.
app.use("/s/*", async (c, next) => {
  const deploymentId = process.env.ALIEN_DEPLOYMENT_ID
  if (!deploymentId) return c.json({ error: "ALIEN_DEPLOYMENT_ID is not set" }, 503)
  if (c.req.header("authorization") !== `Bearer ${deploymentId}`) {
    return c.json({ error: "unauthorized" }, 401)
  }
  await next()
})

interface Body {
  key?: string
  files?: number
  mib?: number
  rounds?: number
  path?: string
  egress?: "none" | "internet"
  sandboxId?: string
}

async function body(c: Context): Promise<Body> {
  const parsed = (await c.req.json()) as Body
  if (parsed.key !== undefined && !KEY.test(parsed.key)) throw new Error("bad key")
  for (const name of ["files", "mib", "rounds"] as const) {
    const value = parsed[name]
    if (value !== undefined && (!Number.isInteger(value) || value < 0 || value > 1_000_000)) {
      throw new Error(`bad ${name}`)
    }
  }
  return parsed
}

function requireKey(b: Body): string {
  if (!b.key) throw new Error("key is required")
  return b.key
}

async function run(sandboxId: string, script: string) {
  let stdout = ""
  let stderr = ""
  let exitCode: number | null = null
  for await (const frame of agents.runCommand(sandboxId, "sh", {
    timeoutMs: COMMAND_MS,
    args: ["-c", script],
  })) {
    if (frame.kind === "exit") exitCode = frame.exitCode
    else if (frame.kind === "stdout") stdout += frame.data.toString()
    else stderr += frame.data.toString()
  }
  return { exitCode, stdout, stderr }
}

/** Opens a sandbox on `key`, runs `script`, syncs, ends the sandbox and reports all of it. */
async function withKey(key: string, script: string, egress?: "none" | "internet") {
  const started = Date.now()
  const session = await agents.create({
    timeoutMs: SANDBOX_MS,
    storageKey: key,
    ...(egress === "none" ? { egress: { allow: [] } } : egress ? { egress: "internet" } : {}),
  })
  const restoredMs = Date.now() - started
  try {
    const result = await run(session.sandboxId, script)
    const synced = await agents.syncStorage(session.sandboxId)
    return { sandboxId: session.sandboxId, restoredMs, ...result, synced }
  } finally {
    await agents.terminate(session.sandboxId)
  }
}

// Builds the round-trip tree under `key` and reports its manifest, synced.
app.post("/s/prepare", async c => {
  const b = await body(c)
  return c.json(await withKey(requireKey(b), prepare(b.files ?? 1000, b.mib ?? 16), b.egress))
})

// Restores `key` in a new sandbox and reports the same manifest plus git, SQLite and link checks.
app.post("/s/verify", async c => {
  const b = await body(c)
  return c.json(await withKey(requireKey(b), VERIFY, b.egress))
})

// What a restore after a kill holds.
app.post("/s/restored", async c => {
  const b = await body(c)
  return c.json(await withKey(requireKey(b), RESTORED, b.egress))
})

// What the command's user can reach, and two symlinks aimed out of the tree for the next sync.
app.post("/s/isolation", async c => {
  const b = await body(c)
  return c.json(await withKey(requireKey(b), ISOLATION, b.egress))
})

// A tree above maxSize: the sync must not publish it.
app.post("/s/oversize", async c => {
  const b = await body(c)
  const key = requireKey(b)
  const session = await agents.create({ timeoutMs: SANDBOX_MS, storageKey: key })
  try {
    const result = await run(session.sandboxId, OVERSIZE)
    let synced: unknown
    try {
      synced = await agents.syncStorage(session.sandboxId)
    } catch (error) {
      synced = { error: error instanceof Error ? error.message : String(error) }
    }
    const status = await agents.storageStatus(session.sandboxId)
    await run(session.sandboxId, "rm -f /workspace/oversize.bin")
    return c.json({ sandboxId: session.sandboxId, ...result, synced, status })
  } finally {
    await agents.terminate(session.sandboxId)
  }
})

// Streams the writer's acknowledged rounds as NDJSON while the test kills the sandbox from outside.
// The first line names the sandbox; the last says how the command ended. No sync and no
// terminate: what the store holds is what the pull loop published before the kill.
app.post("/s/writer", async c => {
  const b = await body(c)
  const key = requireKey(b)
  const session = await agents.create({ timeoutMs: SANDBOX_MS, storageKey: key })
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      const send = (value: unknown) =>
        controller.enqueue(
          encoder.encode(`${JSON.stringify({ ...(value as object), at: Date.now() })}\n`),
        )
      send({ sandboxId: session.sandboxId })
      let pending = ""
      try {
        for await (const frame of agents.runCommand(session.sandboxId, "sh", {
          timeoutMs: COMMAND_MS,
          args: ["-c", writer(b.rounds ?? 3000)],
        })) {
          if (frame.kind === "stdout") {
            pending += frame.data.toString()
            const lines = pending.split("\n")
            pending = lines.pop() ?? ""
            for (const line of lines) if (line) send({ ack: Number(line) })
          } else if (frame.kind === "exit") {
            send({ end: "exit", exitCode: frame.exitCode })
          }
        }
      } catch (error) {
        send({ end: "error", error: error instanceof Error ? error.message : String(error) })
      }
      // Keep the request, and with it the pull loop, open while the binding notices the end.
      await new Promise(resolve => setTimeout(resolve, 5_000))
      controller.close()
    },
  })
  return new Response(stream, { headers: { "content-type": "application/x-ndjson" } })
})

// Two sandboxes on one key: the second claims it, the first must stop and keep its commits out.
app.post("/s/takeover", async c => {
  const b = await body(c)
  const key = requireKey(b)
  const first = await agents.create({ timeoutMs: SANDBOX_MS, storageKey: key })
  try {
    await run(first.sandboxId, "echo first > /workspace/owner")
    await agents.syncStorage(first.sandboxId)
    const second = await agents.create({ timeoutMs: SANDBOX_MS, storageKey: key })
    try {
      await run(second.sandboxId, "echo second > /workspace/owner")
      await agents.syncStorage(second.sandboxId)
      // The stale sandbox writes after losing the key; its sync must not publish.
      await run(first.sandboxId, "echo stale > /workspace/owner")
      let staleSync: unknown
      try {
        staleSync = await agents.syncStorage(first.sandboxId)
      } catch (error) {
        staleSync = { error: error instanceof Error ? error.message : String(error) }
      }
      const firstStatus = await agents.storageStatus(first.sandboxId)
      const owner = (await agents.readStorageFile(key, "owner")).toString()
      return c.json({ firstStatus, staleSync, owner })
    } finally {
      await agents.terminate(second.sandboxId)
    }
  } finally {
    await agents.terminate(first.sandboxId)
  }
})

app.post("/s/status", async c => {
  const b = await body(c)
  if (!b.sandboxId) throw new Error("sandboxId is required")
  return c.json({ status: await agents.storageStatus(b.sandboxId) })
})

// One file of a key as of its last commit, without a sandbox.
app.post("/s/read", async c => {
  const b = await body(c)
  if (!b.path || b.path.includes("..")) throw new Error("bad path")
  try {
    const bytes = await agents.readStorageFile(requireKey(b), b.path)
    return c.json({ found: true, base64: bytes.toString("base64") })
  } catch (error) {
    return c.json({ found: false, error: error instanceof Error ? error.message : String(error) })
  }
})

app.post("/s/delete", async c => {
  const b = await body(c)
  await agents.deleteStorage(requireKey(b))
  return c.json({ deleted: true })
})

export default app
