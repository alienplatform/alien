import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"

// Bun's built-in S3 client, which the Worker runtime provides.
declare const Bun: {
  S3Client: new (options: Record<string, string | undefined>) => {
    write(key: string, body: string): Promise<number>
  }
}

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

// Tries to write one object with this Worker's own credentials, which hold a stack-wide storage
// write grant. The test aims it at the sandbox bundle bucket, where only the deployment's
// management role may write, and at an ordinary bucket as the control.
app.post("/write-object", async c => {
  const deploymentId = process.env.ALIEN_DEPLOYMENT_ID
  if (!deploymentId) return c.json({ error: "ALIEN_DEPLOYMENT_ID is not set" }, 503)
  if (c.req.header("authorization") !== `Bearer ${deploymentId}`) {
    return c.json({ error: "unauthorized" }, 401)
  }
  const { bucket, key } = (await c.req.json()) as { bucket: string; key: string }
  const s3 = new Bun.S3Client({
    bucket,
    region: process.env.AWS_REGION,
    accessKeyId: process.env.AWS_ACCESS_KEY_ID,
    secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY,
    sessionToken: process.env.AWS_SESSION_TOKEN,
  })
  try {
    await s3.write(key, "written by a workload")
    return c.json({ written: true })
  } catch (error) {
    return c.json({ written: false, code: (error as { code?: string }).code ?? String(error) })
  }
})

export default app
