import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"

const app = new Hono()

app.get("/health", c => c.json({ status: "ok" }))

app.onError((error, c) =>
  c.json({ error: error instanceof Error ? (error.stack ?? error.message) : String(error) }, 500),
)

// The endpoint is public, so every operation needs the deployment's own id as the bearer, which
// the test knows and a stranger reaching the URL does not.
app.use("/op/*", async (c, next) => {
  const deploymentId = process.env.ALIEN_DEPLOYMENT_ID
  if (!deploymentId) return c.json({ error: "ALIEN_DEPLOYMENT_ID is not set" }, 503)
  if (c.req.header("authorization") !== `Bearer ${deploymentId}`) {
    return c.json({ error: "unauthorized" }, 401)
  }
  await next()
})

const agents = () => sandbox("agents")

// Opens a sandbox on a storage key; resolves once the key's tree is restored.
app.post("/op/create", async c => {
  const { key } = (await c.req.json()) as { key: string }
  const session = await agents().create({ storageKey: key, timeoutMs: 1_500_000 })
  return c.json({ sandboxId: session.sandboxId })
})

// Runs one command to completion as the image user.
app.post("/op/exec", async c => {
  const { sandboxId, command, args, timeoutMs } = (await c.req.json()) as {
    sandboxId: string
    command: string
    args?: string[]
    timeoutMs?: number
  }
  let stdout = ""
  let stderr = ""
  let exitCode: number | null = null
  for await (const frame of agents().runCommand(sandboxId, command, {
    timeoutMs: timeoutMs ?? 120_000,
    args: args ?? [],
  })) {
    if (frame.kind === "exit") exitCode = frame.exitCode
    else if (frame.kind === "stdout") stdout += frame.data.toString()
    else stderr += frame.data.toString()
  }
  return c.json({ exitCode, stdout, stderr })
})

app.post("/op/status", async c => {
  const { sandboxId } = (await c.req.json()) as { sandboxId: string }
  return c.json({ status: await agents().storageStatus(sandboxId) })
})

app.post("/op/sync", async c => {
  const { sandboxId } = (await c.req.json()) as { sandboxId: string }
  try {
    return c.json({ status: await agents().syncStorage(sandboxId) })
  } catch (error) {
    return c.json({ refused: error instanceof Error ? error.message : String(error) })
  }
})

app.post("/op/terminate", async c => {
  const { sandboxId, discard } = (await c.req.json()) as { sandboxId: string; discard?: boolean }
  await agents().terminate(sandboxId, { discard: discard ?? false })
  return c.json({ terminated: true })
})

app.post("/op/read-storage-file", async c => {
  const { key, path } = (await c.req.json()) as { key: string; path: string }
  try {
    const bytes = await agents().readStorageFile(key, path)
    return c.json({ base64: bytes.toString("base64") })
  } catch (error) {
    return c.json({ refused: error instanceof Error ? error.message : String(error) })
  }
})

app.post("/op/delete-storage", async c => {
  const { key } = (await c.req.json()) as { key: string }
  try {
    await agents().deleteStorage(key)
    return c.json({ deleted: true })
  } catch (error) {
    return c.json({ refused: error instanceof Error ? error.message : String(error) })
  }
})

export default app
