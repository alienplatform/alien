import { describe, expect, it } from "vitest"
import type { DataPlane, RunnerEvent } from "../control-plane/src/data-plane"
import { ThreadRouter, type UiEvent } from "../control-plane/src/router"
import { ThreadStore } from "../control-plane/src/store"

// A data plane that records the order of calls and plays back one short turn.
function fakeDataPlane(options: { pauseResume: boolean }) {
  const calls: string[] = []
  let paused = false
  let events: RunnerEvent[] = []
  const dp: DataPlane = {
    async open(_threadId, sandboxId) {
      calls.push(`open ${sandboxId ?? "-"}`)
      if (sandboxId && paused) {
        paused = false
        return { sandboxId, created: false, resumed: true }
      }
      return { sandboxId: sandboxId ?? "sbx-1", created: !sandboxId, resumed: false }
    },
    async issueToken(threadId, user) {
      calls.push(`issue-token ${threadId} ${user}`)
      return { token: `tok-${user}`, expiresAt: 0 }
    },
    async bindTurn(sandboxId, token) {
      calls.push(`bind-turn ${sandboxId} ${token}`)
      return { bound: true }
    },
    async turn(input) {
      calls.push(`turn ${input.sandboxId} ${input.sessionId ?? "-"} ${input.user}`)
      events = [
        { seq: 1, kind: "agent", event: { type: "system", subtype: "init", session_id: "sess-1" } },
        { seq: 2, kind: "agent", event: { type: "result" } },
        { seq: 3, kind: "turn-end", exitCode: 0 },
      ]
    },
    async events(_threadId, after) {
      return { events: events.filter(e => e.seq > after), running: false, sessionId: "sess-1" }
    },
    async exec() {
      return { exitCode: 0, stdout: "", stderr: "" }
    },
    async pause(_threadId, sandboxId) {
      calls.push(`pause ${sandboxId}`)
      if (!options.pauseResume) return { paused: false, reason: "no pause here" }
      paused = true
      return { paused: true }
    },
    async approvals() {
      return []
    },
    async decide() {
      return true
    },
  }
  return { dp, calls }
}

async function until(check: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !check(); i++) await new Promise(r => setTimeout(r, 5))
  expect(check()).toBe(true)
}

describe("thread router", () => {
  it("binds the sandbox to the asking user before the turn, and resumes the session", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    await router.send(thread.id, "alice", "list orders")
    await until(() => store.get(thread.id)?.state === "idle")
    await router.send(thread.id, "bob", "refund 1003")
    await until(() => store.get(thread.id)?.state === "idle")

    expect(calls).toEqual([
      "open -",
      `issue-token ${thread.id} alice`,
      "bind-turn sbx-1 tok-alice",
      "turn sbx-1 - alice",
      "open sbx-1",
      `issue-token ${thread.id} bob`,
      "bind-turn sbx-1 tok-bob",
      "turn sbx-1 sess-1 bob",
    ])
  })

  it("pauses an idle thread and resumes it on the next message", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 10, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))

    await router.send(thread.id, "alice", "hello")
    await until(() => store.get(thread.id)?.state === "paused")
    await router.send(thread.id, "alice", "still there?")

    expect(calls).toContain("pause sbx-1")
    expect(seen).toContainEqual({ type: "notice", text: "resumed sandbox sbx-1" })
  })

  it("a refused bind stops the turn from starting", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    dp.bindTurn = async () => {
      throw new Error("gateway did not acknowledge")
    }
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    await expect(router.send(thread.id, "alice", "hello")).rejects.toThrow("gateway did not acknowledge")
    expect(calls.some(c => c.startsWith("turn"))).toBe(false)
    expect(store.get(thread.id)?.state).toBe("idle")
  })
})
