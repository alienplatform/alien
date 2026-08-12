import type { Sandbox } from "@alienplatform/sdk"
import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/sandbox-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  const box = sandbox(bindingName)
  const marker = `alien-sandbox-e2e-${Date.now()}`

  const session = await box.create({ sessionId: `e2e-ts-${Date.now()}` }).catch(async error => {
    throw await toExternalOperationError(error, "sandbox-test")
  })

  // Both run before either is reported, and a leaked session is reported first even when the
  // exercise also failed: an exercise failure is a broken test, a surviving session is a billable
  // sandbox nobody will look for.
  const outcome = await attempt(() => exercise(box, session.sessionId, marker))
  const cleanup = await attempt(() => terminateAndConfirm(box, session.sessionId))

  const failure = cleanup ?? outcome
  if (failure) {
    return c.json({ success: false, error: failure }, 500)
  }

  return c.json({ success: true, bindingName })
})

/** Runs `step`, returning why it failed rather than throwing, so both steps always run. */
async function attempt(step: () => Promise<string | null>): Promise<string | null> {
  try {
    return await step()
  } catch (error: unknown) {
    const alienError = await toExternalOperationError(error, "sandbox-test")
    return `${alienError.code}: ${alienError.message}`
  }
}

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

  return null
}

/** How long to wait for a terminate to converge before calling the session leaked. */
const TERMINATE_POLL_ATTEMPTS = 15
const TERMINATE_POLL_INTERVAL_MS = 2000

/**
 * Terminates the session and reads it back to confirm it is gone.
 *
 * A successful terminate is not the same claim: the backends return once deletion is accepted,
 * so a test that stops at the return value passes while the session keeps running.
 */
async function terminateAndConfirm(box: Sandbox, sessionId: string): Promise<string | null> {
  await box.terminate(sessionId)

  // Polled rather than read once: every backend returns from terminate as soon as the deletion is
  // accepted, so a single read races normal convergence and would fail a teardown that was simply
  // still finishing.
  // A failed read inside the window is retried like a non-terminal state: the session may well be
  // gone, and giving up on the first blip would fail a teardown that had already converged. A read
  // that never succeeds still fails, carrying the last error rather than a bare timeout.
  let last = "unread"
  for (let attempt = 0; attempt < TERMINATE_POLL_ATTEMPTS; attempt++) {
    try {
      const remaining = await box.get(sessionId)
      if (remaining === null || remaining.state === "terminated") {
        return null
      }
      last = remaining.state
    } catch (error: unknown) {
      last = `unreadable (${error instanceof Error ? error.message : String(error)})`
    }
    if (attempt + 1 < TERMINATE_POLL_ATTEMPTS) {
      await new Promise(resolve => setTimeout(resolve, TERMINATE_POLL_INTERVAL_MS))
    }
  }

  const waited = (TERMINATE_POLL_ATTEMPTS * TERMINATE_POLL_INTERVAL_MS) / 1000
  return `session '${sessionId}' is still ${last} ${waited}s after terminate; it may still be billing`
}

export default app
