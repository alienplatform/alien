import { describe, expect, it } from "vitest"
import type { DataPlane, RunnerEvent } from "../control-plane/src/data-plane"
import { ThreadRouter, type UiEvent } from "../control-plane/src/router"
import { ThreadStore } from "../control-plane/src/store"

// A data plane that records the order of calls and plays back one short turn.
function fakeDataPlane(options: { pauseResume: boolean; pauseMs?: number }) {
  const calls: string[] = []
  let paused = false
  let running = false
  let created = 0
  let events: RunnerEvent[] = []
  const dp: DataPlane = {
    async open(_threadId, sandboxId) {
      calls.push(`open ${sandboxId ?? "-"}`)
      await new Promise(r => setTimeout(r, 5))
      if (sandboxId && paused) {
        paused = false
        return { sandboxId, created: false, resumed: true }
      }
      if (sandboxId) return { sandboxId, created: false, resumed: false }
      created += 1
      return { sandboxId: `sbx-${created}`, created: true, resumed: false }
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
      // A turn that is still running has not produced its end yet.
      if (running) return
      events = [
        { seq: 1, kind: "agent", event: { type: "system", subtype: "init", session_id: "sess-1" } },
        { seq: 2, kind: "agent", event: { type: "result" } },
        { seq: 3, kind: "turn-end", exitCode: 0 },
      ]
    },
    async events(_threadId, after) {
      await new Promise(r => setTimeout(r, 1))
      return { events: events.filter(e => e.seq > after), running, sessionId: "sess-1" }
    },
    async endTurn(threadId, sandboxId) {
      calls.push(`end-turn ${threadId} ${sandboxId}`)
    },
    async endProcesses(sandboxId, user) {
      calls.push(`end-processes ${sandboxId} ${user}`)
      return { ended: 2 }
    },
    async exec(_sandboxId, _command, user) {
      calls.push(`exec ${user}`)
      return { exitCode: 0, stdout: "", stderr: "" }
    },
    async pause(_threadId, sandboxId) {
      calls.push(`pause ${sandboxId} start`)
      await new Promise(r => setTimeout(r, options.pauseMs ?? 0))
      calls.push(`pause ${sandboxId} end`)
      if (!options.pauseResume) return { paused: false, reason: "no pause here" }
      paused = true
      return { paused: true }
    },
    async approvals() {
      return []
    },
    async decide() {
      return { decided: true }
    },
  }
  return { dp, calls, setRunning: (value: boolean) => (running = value) }
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
      `end-turn ${thread.id} sbx-1`,
      "open sbx-1",
      "end-processes sbx-1 alice",
      `issue-token ${thread.id} bob`,
      "bind-turn sbx-1 tok-bob",
      "turn sbx-1 sess-1 bob",
      `end-turn ${thread.id} sbx-1`,
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

    expect(calls).toContain("pause sbx-1 end")
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
    // The token that was issued does not outlive the failed start.
    expect(calls).toContain(`end-turn ${thread.id} sbx-1`)
  })

  it("bound: false stops the turn and the thread shows why", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    dp.bindTurn = async () => ({ bound: false, reason: "no credentials API" })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))

    await expect(router.send(thread.id, "alice", "hello")).rejects.toThrow("not bound to alice: no credentials API")
    expect(calls.some(c => c.startsWith("turn"))).toBe(false)
    expect(seen).toContainEqual({
      type: "notice",
      text: "turn failed to start: the sandbox was not bound to alice: no credentials API",
    })
  })

  it("clears the turn's access when the turn ends", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    await router.send(thread.id, "alice", "hello")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(calls.indexOf(`end-turn ${thread.id} sbx-1`)).toBeGreaterThan(calls.indexOf("turn sbx-1 - alice"))
  })

  it("a user switch ends the previous user's processes before binding; the same user keeps them", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))

    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "idle")
    await router.send(thread.id, "alice", "two") // same user: nothing is ended
    await until(() => store.get(thread.id)?.state === "idle")
    expect(calls.filter(c => c.startsWith("end-processes"))).toEqual([])

    await router.send(thread.id, "bob", "three")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(calls.filter(c => c.startsWith("end-processes"))).toEqual(["end-processes sbx-1 alice"])
    const ended = calls.indexOf("end-processes sbx-1 alice")
    expect(ended).toBeLessThan(calls.indexOf(`issue-token ${thread.id} bob`))
    expect(seen).toContainEqual({ type: "notice", text: "ended 2 processes that alice's turns left running" })
  })

  it("a message that arrives during a pause waits for it, then resumes the sandbox", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true, pauseMs: 60 })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 10, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    await router.send(thread.id, "alice", "hello")
    await until(() => calls.includes("pause sbx-1 start"))
    expect(calls).not.toContain("pause sbx-1 end")
    const second = router.send(thread.id, "alice", "back")
    await until(() => calls.includes("pause sbx-1 end"))
    await second
    // The pause finished before the message looked at the sandbox, and the message resumed it.
    expect(calls.indexOf("pause sbx-1 end")).toBeLessThan(calls.lastIndexOf("open sbx-1"))
    expect(calls.filter(c => c === "open sbx-1").length).toBeGreaterThan(0)
    await until(() => store.get(thread.id)?.state === "idle")
  })

  it("does not pause while a turn is running", async () => {
    const { dp, calls, setRunning } = fakeDataPlane({ pauseResume: true })
    setRunning(true) // the runner keeps reporting a running turn
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 5, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    await router.send(thread.id, "alice", "long job")
    await new Promise(r => setTimeout(r, 60))
    expect(store.get(thread.id)?.state).toBe("running")
    expect(calls.some(c => c.startsWith("pause"))).toBe(false)
    // Once the runner says the turn is over, the thread goes idle and is paused.
    setRunning(false)
    await until(() => store.get(thread.id)?.state === "paused")
  })

  it("two first messages at once give one sandbox", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")

    const results = await Promise.allSettled([
      router.send(thread.id, "alice", "one"),
      router.send(thread.id, "alice", "two"),
    ])
    expect(results.filter(r => r.status === "fulfilled")).toHaveLength(1)
    expect(calls.filter(c => c === "open -")).toHaveLength(1)
    expect(store.get(thread.id)?.sandboxId).toBe("sbx-1")
  })
})
