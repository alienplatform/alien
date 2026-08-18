import type { Sandbox, SandboxSession } from "@alienplatform/sdk"
import { sandbox } from "@alienplatform/sdk"
import { Hono } from "hono"
import { toExternalOperationError } from "../helpers.js"

const app = new Hono()

/** How long the whole check may take, teardown included. Held under the 255s idle timeout the
 *  app's own server sets in `packages/sdk/src/worker-runtime/index.ts`: this handler answers only
 *  at the end, so a longer run reaches the caller as a proxy error carrying none of the detail. */
const REQUEST_BUDGET_MS = 150_000
/** Held back from the budget so teardown still runs after a slow create or exercise. Local frees
 *  a session only when the sandbox itself is deleted, so skipping teardown strands it. */
const TEARDOWN_RESERVE_MS = 45_000
const POLL_INTERVAL_MS = 2_000

app.post("/sandbox-test/:bindingName", async c => {
  const bindingName = c.req.param("bindingName")
  const box = sandbox(bindingName)
  const marker = `alien-sandbox-e2e-${Date.now()}`
  // Chosen here so teardown can name a session even when create never reports one. Local honours
  // a requested id, and Local is the only platform this check runs on.
  const sessionId = `e2e-ts-${Date.now()}`

  const exerciseBy = Date.now() + REQUEST_BUDGET_MS - TEARDOWN_RESERVE_MS
  const left = () => Math.max(exerciseBy - Date.now(), 1)

  let session: SandboxSession | undefined
  let failed: string | null = null
  // `within` gives up on its own timer without cancelling the call, so whether the create settled
  // is the thing teardown needs to know, not whether this await returned.
  let createSettled = false
  const creating = box.create({ sessionId }).finally(() => {
    createSettled = true
  })
  try {
    session = await within(creating, left())
  } catch (error: unknown) {
    failed = await describe(error)
    if (!createSettled) {
      failed = `${failed}; a session it lands later is reaped when the sandbox is deleted`
    }
  }

  if (session) {
    const id = session.sessionId
    failed = await attempt(() => within(exercise(box, id, marker), left()))
  }

  // Teardown runs whatever happened above, and both failures are reported: a session left running
  // is a sandbox nobody will look for, and why the exercise failed is often why it is still there.
  const leaked = await attempt(() => converge(box, session?.sessionId ?? sessionId, createSettled))
  const failure = [leaked, failed].filter(Boolean).join("; ")
  if (failure) {
    return c.json({ success: false, error: failure }, 500)
  }

  return c.json({ success: true, bindingName })
})

/** Renders a failure the way the e2e runner reports it. */
async function describe(error: unknown): Promise<string> {
  const alienError = await toExternalOperationError(error, "sandbox-test")
  return `${alienError.code}: ${alienError.message}`
}

/** Runs `step`, returning why it failed rather than throwing, so teardown always runs. */
async function attempt(step: () => Promise<string | null>): Promise<string | null> {
  try {
    return await step()
  } catch (error: unknown) {
    return await describe(error)
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

/**
 * Terminates the session and reads it back until it is gone.
 *
 * Terminate is reissued on every pass: it is idempotent, a transient refusal is the one failure
 * that leaves a session running, and a session visible only on a later pass is ended rather than
 * reported as a leak.
 *
 * `createSettled` is false when create never answered. The session can still be created after an
 * empty read there, so absence decides nothing and the wait runs its full reserve, ending whatever
 * appears. A read that fails counts as present: an unreadable session is not a gone one.
 */
async function converge(
  box: Sandbox,
  sessionId: string,
  createSettled: boolean,
): Promise<string | null> {
  const deadline = Date.now() + TEARDOWN_RESERVE_MS
  const left = () => Math.max(deadline - Date.now(), 1)

  for (;;) {
    let refused: string | null = null
    try {
      await within(box.terminate(sessionId), left())
    } catch (error: unknown) {
      refused = error instanceof Error ? error.message : String(error)
    }

    let present: string | null
    try {
      const session = await within(box.get(sessionId), left())
      present = session === null || session.state === "terminated" ? null : session.state
    } catch (error: unknown) {
      present = `unreadable (${error instanceof Error ? error.message : String(error)})`
    }

    if (present === null && createSettled) {
      return null
    }

    if (Date.now() >= deadline) {
      // Nothing left to end. A create still in flight can land one after this, which is why the
      // caller reports its own failure and the sandbox's deletion reaps what remains.
      if (present === null) {
        return null
      }
      const why = refused === null ? present : `${present}, terminate refused: ${refused}`
      const waited = Math.round(TEARDOWN_RESERVE_MS / 1000)
      return `session '${sessionId}' is still ${why} ${waited}s after terminate`
    }
    await new Promise(resolve => setTimeout(resolve, POLL_INTERVAL_MS))
  }
}

/** Rejects when an operation gives no answer in time, so one stall cannot spend the whole budget. */
function within<T>(operation: Promise<T>, timeoutMs: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const expiry = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`no answer in ${Math.round(Math.max(timeoutMs, 0) / 1000)}s`)),
      timeoutMs,
    )
  })
  return Promise.race([operation, expiry]).finally(() => clearTimeout(timer))
}

export default app
