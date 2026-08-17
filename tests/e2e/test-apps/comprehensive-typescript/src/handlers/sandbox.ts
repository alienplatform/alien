import type { Sandbox } from "@alienplatform/sdk"
import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

app.post("/sandbox-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  const box = sandbox(bindingName)
  const marker = `alien-sandbox-e2e-${Date.now()}`

  // Bounded, like everything after it: a stalled create is a failed test, not a hung suite.
  // The id is chosen here so a create that answers late can still be ended by name.
  const sessionId = `e2e-ts-${Date.now()}`
  const session = await within(box.create({ sessionId }), CREATE_TIMEOUT_MS).catch(async error => {
    // The create may still complete after this returns; ending the chosen id makes that a
    // no-op session rather than a leak, and a refusal here is reported over the timeout.
    const cleanup = await attempt(() => terminateAndConfirm(box, sessionId))
    if (cleanup) {
      throw new Error(cleanup)
    }
    throw await toExternalOperationError(error, "sandbox-test")
  })

  // Both run before either is reported, and a leaked session is reported first even when the
  // exercise also failed: an exercise failure is a broken test, a surviving session is a billable
  // sandbox nobody will look for. The exercise is bounded as a whole so a command or file
  // operation that never answers still lets terminate run.
  const outcome = await attempt(() =>
    within(exercise(box, session.sessionId, marker), EXERCISE_TIMEOUT_MS),
  )
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

/** How long a create may take. Longer than a command: on AWS a session waits for the MicroVM
 *  to answer before create returns. */
const CREATE_TIMEOUT_MS = 120_000
/** How long the whole exercise — commands and files — may take before terminate runs anyway. */
const EXERCISE_TIMEOUT_MS = 180_000
/** How many times a refused terminate is retried before the session is called leaked. */
const TERMINATE_ATTEMPTS = 5
/** How long to wait for an accepted terminate to converge before calling the session leaked. */
const TERMINATE_POLL_ATTEMPTS = 15
const TERMINATE_POLL_INTERVAL_MS = 2000
/** How long one status read may take before it counts as a failed read: a stalled manager must
 *  cost one attempt, not the whole test. */
const STATUS_READ_TIMEOUT_MS = 10_000
/** How long one terminate call may take before it counts as refused. Longer than a read: a
 *  backend may confirm deletion inside the call, and that is bounded on its side too. */
const TERMINATE_CALL_TIMEOUT_MS = 60_000

/** Rejects when the operation gives no answer in time, so a stall costs one attempt. */
function within<T>(operation: Promise<T>, timeoutMs: number): Promise<T> {
  return Promise.race([
    operation,
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`no answer in ${timeoutMs / 1000}s`)), timeoutMs),
    ),
  ])
}

/**
 * Terminates the session and reads it back to confirm it is gone.
 *
 * A successful terminate is not the same claim: the backends return once deletion is accepted,
 * so a test that stops at the return value passes while the session keeps running.
 */
async function terminateAndConfirm(box: Sandbox, sessionId: string): Promise<string | null> {
  // Polled rather than read once: every backend returns from terminate as soon as the deletion is
  // accepted, so a single read races normal convergence and would fail a teardown that was simply
  // still finishing.
  // The terminate itself is retried, because it is idempotent and a transient refusal is the one
  // failure that leaves a session running: giving up on it would hand back an error and a
  // billable sandbox nobody will look for.
  // A failed read inside the window is retried like a non-terminal state: the session may well be
  // gone, and giving up on the first blip would fail a teardown that had already converged. A read
  // that never succeeds still fails, carrying the last error rather than a bare timeout.
  // The two budgets are separate so a terminate accepted on its last try still gets the whole
  // convergence window rather than one immediate read.
  let refused: string | null = null
  for (let attempt = 0; attempt < TERMINATE_ATTEMPTS; attempt++) {
    try {
      await within(box.terminate(sessionId), TERMINATE_CALL_TIMEOUT_MS)
      refused = null
      break
    } catch (error: unknown) {
      refused = error instanceof Error ? error.message : String(error)
      if (attempt + 1 < TERMINATE_ATTEMPTS) {
        await new Promise(resolve => setTimeout(resolve, TERMINATE_POLL_INTERVAL_MS))
      }
    }
  }
  // A refused terminate is not yet a leak: the request may have succeeded with its response
  // lost, so the confirmation poll below decides, and the refusal is what is reported if the
  // session turns out to still be there.
  let last = refused === null ? "unread" : `running (terminate refused: ${refused})`
  for (let attempt = 0; attempt < TERMINATE_POLL_ATTEMPTS; attempt++) {
    try {
      const remaining = await within(box.get(sessionId), STATUS_READ_TIMEOUT_MS)
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

  if (refused !== null) {
    return `session '${sessionId}' could not be terminated (${refused}); it may still be billing`
  }
  const waited = (TERMINATE_POLL_ATTEMPTS * TERMINATE_POLL_INTERVAL_MS) / 1000
  return `session '${sessionId}' is still ${last} ${waited}s after terminate; it may still be billing`
}

export default app
