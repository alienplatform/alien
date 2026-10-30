import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

app.get("/health", c => c.json({ status: "ok" }))

// A failure reports itself, so a broken session names its cause instead of a bare 500.
app.onError((error, c) =>
  c.json({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) }, 500),
)

// Opens a session, runs one shell command in it, and ends the session.
app.post("/run", async c => {
  const { command } = (await c.req.json()) as { command: string }
  const agents = sandbox("agents")
  const session = await agents.create({ timeoutMs: 300_000 })
  let stdout = ""
  let stderr = ""
  let exitCode: number | null = null
  try {
    for await (const frame of agents.runCommand(session.sandboxId, "/bin/sh", {
      timeoutMs: 60_000,
      args: ["-c", command],
    })) {
      if (frame.kind === "exit") exitCode = frame.exitCode
      else if (frame.kind === "stdout") stdout += frame.data.toString()
      else stderr += frame.data.toString()
    }
  } finally {
    await agents.terminate(session.sandboxId)
  }
  return c.json({ sandboxId: session.sandboxId, exitCode, stdout, stderr })
})

export default app
