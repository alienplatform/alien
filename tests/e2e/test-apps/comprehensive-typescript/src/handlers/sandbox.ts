import type { Sandbox, SandboxSession } from "@alienplatform/sdk"
import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

// No timeouts here: the runner bounds the whole request, and a step that hangs is a failed check
// either way. A bound inside the handler would only add a path where the handler abandons a
// session it then has to hunt for — and the runtime drops this handler when the runner gives up,
// so cleanup cannot depend on it surviving. The sandbox's own delete flow reaps every session, and
// the runner asserts none survived; this handler only has to leave nothing behind when it is the
// one still running.
app.post("/sandbox-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  const box = sandbox(bindingName)
  const marker = `alien-sandbox-e2e-${Date.now()}`

  const sessionId = `e2e-ts-${Date.now()}`
  let session: SandboxSession
  try {
    session = await box.create({ sessionId })
  } catch (error: unknown) {
    // A create that provisioned and then lost its answer exists under the id asked for on every
    // backend that honours one, so ending that id here is what stops it; where the backend
    // allocates its own id there is nothing to name, and its teardown reaps it.
    await box.terminate(sessionId).catch(() => undefined)
    const alienError = await toExternalOperationError(error, "sandbox-test")
    return c.json({ success: false, error: `${alienError.code}: ${alienError.message}` }, 500)
  }

  let failure: string | null
  try {
    failure = await exercise(box, session.sessionId, marker)
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "sandbox-test")
    failure = `${alienError.code}: ${alienError.message}`
  }

  // Idempotent on every backend, so a failed exercise and a healthy one tear down the same way.
  // Its own failure is reported only when it is the sole failure: the fault the exercise found is
  // what a reader of this check needs, and a session that also would not close is second to it.
  try {
    await box.terminate(session.sessionId)
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "sandbox-terminate")
    failure ??= `${alienError.code}: ${alienError.message}`
  }

  if (failure) {
    return c.json({ success: false, error: failure }, 500)
  }
  return c.json({ success: true, bindingName })
})

// What the check leaves behind, read from the backend rather than trusted from the handler: a
// session the handler abandoned would not show up in its own answer. Where the backend cannot
// enumerate, that is reported as such rather than as zero.
app.get("/sandbox-sessions/:bindingName", async c => {
  const box = sandbox(c.req.param("bindingName"))
  try {
    const sessions = await box.list()
    return c.json({ enumerable: true, sessionIds: sessions.map(s => s.sessionId) })
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "sandbox-sessions")
    if (alienError.context?.sourceCode === "OPERATION_NOT_SUPPORTED") {
      return c.json({ enumerable: false, sessionIds: [] })
    }
    return c.json({ success: false, error: `${alienError.code}: ${alienError.message}` }, 500)
  }
})

/** Runs a command and moves a file both directions through one session. */
async function exercise(box: Sandbox, sessionId: string, marker: string): Promise<string | null> {
  let stdout = ""
  let stderr = ""
  let exitCode: number | undefined

  for await (const frame of box.runCommand(sessionId, ["/bin/echo", marker], {
    deadlineMs: 30_000,
  })) {
    if (frame.kind === "stdout") stdout += frame.data.toString("utf8")
    if (frame.kind === "stderr") stderr += frame.data.toString("utf8")
    if (frame.kind === "exit") exitCode = frame.exitCode
  }

  if (exitCode !== 0) {
    // stderr is kept, not reduced to a boolean: when this fails it is the only thing that says
    // why, and this harness diagnoses a deployment from the outside.
    return `command exited with ${exitCode}: ${stderr}`
  }
  if (!stdout.includes(marker)) {
    return `stdout did not carry the marker: ${stdout}`
  }

  // Files both directions through the same session, which is what makes it a session rather
  // than a sequence of unrelated commands.
  await box.writeFiles(sessionId, { "e2e/input.txt": marker })
  const readBack = await box.readFile(sessionId, "e2e/input.txt")
  if (readBack.toString("utf8") !== marker) {
    return "readFile returned different bytes than writeFiles sent"
  }

  // Read the same file from inside the session, not only back through the agent. `readFile` is
  // the agent reading what the agent wrote, so it holds even when the command that the upload
  // exists for cannot open it — which is the difference between the backends that run an agent
  // as a different user than the command and the ones that do not.
  let fromInside = ""
  let insideStderr = ""
  let insideExit: number | undefined
  for await (const frame of box.runCommand(sessionId, ["/bin/cat", "e2e/input.txt"], {
    deadlineMs: 30_000,
  })) {
    if (frame.kind === "stdout") fromInside += frame.data.toString("utf8")
    if (frame.kind === "stderr") insideStderr += frame.data.toString("utf8")
    if (frame.kind === "exit") insideExit = frame.exitCode
  }
  if (insideExit !== 0) {
    return `the session could not read the file written into it, exit ${insideExit}: ${insideStderr}`
  }
  if (!fromInside.includes(marker)) {
    return `the session read different bytes than writeFiles sent: ${fromInside}`
  }

  return null
}

export default app
