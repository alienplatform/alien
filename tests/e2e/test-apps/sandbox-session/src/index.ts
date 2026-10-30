import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

app.get("/health", c => c.json({ status: "ok" }))

// A failure reports itself, so a broken session names its cause instead of a bare 500.
app.onError((error, c) =>
  c.json({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) }, 500),
)

// Opens a session, reads the marker the image was built with, and ends the session. The endpoint
// is public, so the command is fixed and the caller must present the deployment's own id, which
// the test knows and a stranger reaching the URL does not.
app.post("/run", async c => {
  const deploymentId = process.env.ALIEN_DEPLOYMENT_ID
  if (!deploymentId) return c.json({ error: "ALIEN_DEPLOYMENT_ID is not set" }, 503)
  if (c.req.header("authorization") !== `Bearer ${deploymentId}`) {
    return c.json({ error: "unauthorized" }, 401)
  }
  const agents = sandbox("agents")
  const session = await agents.create({ timeoutMs: 300_000 })
  let stdout = ""
  let stderr = ""
  let exitCode: number | null = null
  try {
    for await (const frame of agents.runCommand(session.sandboxId, "cat", {
      timeoutMs: 60_000,
      args: ["/etc/sandbox-session-version"],
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
