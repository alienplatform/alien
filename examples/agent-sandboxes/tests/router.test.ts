import { describe, expect, it } from "vitest"
import type { DataPlane, RunnerEvent } from "../control-plane/src/data-plane"
import { ThreadRouter, type UiEvent } from "../control-plane/src/router"
import { ThreadStore } from "../control-plane/src/store"

// A data plane that records the order of calls and plays back one short turn.
function fakeDataPlane(options: { pauseResume: boolean; pauseMs?: number; execMs?: number }) {
  const calls: string[] = []
  let paused = false
  let running = false
  let created = 0
  let events: RunnerEvent[] = []
  let generation = "gen-1"
  let override: RunnerEvent[] | undefined
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
      calls.push(`turn ${input.sandboxId} ${input.sessionId ?? "-"}`)
      // A turn that is still running has not produced its end yet.
      if (running) return { turnId: "turn-1" }
      if (override) {
        events = override
        return { turnId: "turn-1" }
      }
      events = [
        { seq: 1, kind: "agent", event: { type: "system", subtype: "init", session_id: "sess-1" } },
        { seq: 2, kind: "agent", event: { type: "result" } },
        { seq: 3, kind: "turn-end", exitCode: 0 },
      ]
      return { turnId: "turn-1" }
    },
    async events(_threadId, after) {
      await new Promise(r => setTimeout(r, 1))
      return {
        events: events.filter(e => e.seq > after),
        running,
        sessionId: "sess-1",
        generation,
      }
    },
    async endTurn(threadId, sandboxId) {
      calls.push(`end-turn ${threadId} ${sandboxId}`)
    },
    async stopTurn(threadId, turnId) {
      calls.push(`stop-turn ${threadId} ${turnId}`)
      running = false
      return { stopped: true, running }
    },
    async endProcesses(sandboxId) {
      calls.push(`end-processes ${sandboxId}`)
      return { ended: 2 }
    },
    async exec(sandboxId, command) {
      calls.push(`exec ${sandboxId} ${command} start`)
      await new Promise(r => setTimeout(r, options.execMs ?? 0))
      calls.push(`exec ${sandboxId} ${command} end`)
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
  return {
    dp,
    calls,
    setRunning: (value: boolean) => (running = value),
    setGeneration: (value: string) => (generation = value),
    setEvents: (value: RunnerEvent[]) => (override = value),
  }
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
      "turn sbx-1 -",
      `end-turn ${thread.id} sbx-1`,
      "open sbx-1",
      "end-processes sbx-1",
      `issue-token ${thread.id} bob`,
      "bind-turn sbx-1 tok-bob",
      "turn sbx-1 sess-1",
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

    await expect(router.send(thread.id, "alice", "hello")).rejects.toThrow(
      "gateway did not acknowledge",
    )
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

    await expect(router.send(thread.id, "alice", "hello")).rejects.toThrow(
      "not bound to alice: no credentials API",
    )
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
    expect(calls.indexOf(`end-turn ${thread.id} sbx-1`)).toBeGreaterThan(
      calls.indexOf("turn sbx-1 -"),
    )
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
    expect(calls.filter(c => c.startsWith("end-processes"))).toEqual(["end-processes sbx-1"])
    const ended = calls.indexOf("end-processes sbx-1")
    expect(ended).toBeLessThan(calls.indexOf(`issue-token ${thread.id} bob`))
    expect(seen).toContainEqual({
      type: "notice",
      text: "ended 2 processes that alice's turns left running",
    })
  })

  it("a failed sweep stops the turn, and the retry sweeps again", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "idle")

    const sweep = dp.endProcesses
    dp.endProcesses = async sandboxId => {
      calls.push(`end-processes ${sandboxId} (fails)`)
      throw new Error("1 process still running after the sweep")
    }
    await expect(router.send(thread.id, "bob", "two")).rejects.toThrow("still running")
    expect(calls.filter(c => c.startsWith("turn "))).toHaveLength(1)
    expect(store.get(thread.id)?.owner).toBe("alice")
    expect(store.get(thread.id)?.user).toBe("bob")

    dp.endProcesses = sweep
    await router.send(thread.id, "bob", "two again")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(calls).toContain("end-processes sbx-1")
    expect(calls.filter(c => c.startsWith("turn "))).toHaveLength(2)
    expect(store.get(thread.id)?.owner).toBe("bob")
  })

  it("a thread saved without an owner takes the previous user as owner, so a failed first sweep is retried", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "idle")
    store.update(thread.id, { owner: undefined })

    dp.endProcesses = async () => {
      throw new Error("1 process still running after the sweep")
    }
    await expect(router.send(thread.id, "bob", "two")).rejects.toThrow("still running")
    expect(store.get(thread.id)?.owner).toBe("alice")

    dp.endProcesses = async sandboxId => {
      calls.push(`end-processes ${sandboxId}`)
      return { ended: 1 }
    }
    await router.send(thread.id, "bob", "two again")
    expect(calls).toContain("end-processes sbx-1")
  })

  it("a user switch waits for a running exec and sweeps after it", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true, execMs: 40 })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "idle")

    const exec = router.exec(thread.id, "sleep 99 &")
    await until(() => calls.includes("exec sbx-1 sleep 99 & start"))
    const send = router.send(thread.id, "bob", "two")
    await exec
    await send
    expect(calls.indexOf("end-processes sbx-1")).toBeGreaterThan(
      calls.indexOf("exec sbx-1 sleep 99 & end"),
    )
  })

  it("does not pause while an exec runs, and pauses after it once idle again", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true, execMs: 60 })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 20, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "paused")
    calls.length = 0

    await router.exec(thread.id, "ls")
    expect(calls.filter(c => c.startsWith("pause"))).toEqual([])
    await until(() => calls.includes("pause sbx-1 end"))
    expect(calls.indexOf("pause sbx-1 end")).toBeGreaterThan(calls.indexOf("exec sbx-1 ls end"))
  })

  it("a poll error does not end a turn that may still run; polling resumes", async () => {
    const { dp, calls, setRunning } = fakeDataPlane({ pauseResume: true })
    const real = dp.events
    let failing = 3
    dp.events = async (...args) => {
      if (failing-- > 0) throw new Error("command timed out")
      return real(...args)
    }
    setRunning(false)
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, {
      idlePauseMs: 60_000,
      pollMs: 1,
      retryMs: 1,
      giveUpMs: 60_000,
    })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))
    await router.send(thread.id, "alice", "hello")
    await new Promise(r => setTimeout(r, 5))
    expect(store.get(thread.id)?.state).toBe("running")
    expect(calls.some(c => c.startsWith("end-turn"))).toBe(false)
    await until(() => store.get(thread.id)?.state === "idle")
    expect(seen.some(e => e.type === "agent")).toBe(true)
  })

  it("after the give-up time the turn is stopped through the runner", async () => {
    const { dp, calls, setRunning } = fakeDataPlane({ pauseResume: true })
    dp.events = async () => {
      throw new Error("command timed out")
    }
    setRunning(true)
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, {
      idlePauseMs: 60_000,
      pollMs: 1,
      retryMs: 1,
      giveUpMs: 20,
    })
    const thread = router.create("Orders", "alice")
    await router.send(thread.id, "alice", "hello")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(calls).toContain(`stop-turn ${thread.id} turn-1`)
    expect(calls).toContain(`end-turn ${thread.id} sbx-1`)
  })

  it("if the runner cannot be reached at all, the thread stays running with access intact", async () => {
    const { dp, calls, setRunning } = fakeDataPlane({ pauseResume: true })
    dp.events = async () => {
      throw new Error("command timed out")
    }
    dp.stopTurn = async () => {
      throw new Error("no runner")
    }
    setRunning(true)
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, {
      idlePauseMs: 60_000,
      pollMs: 1,
      retryMs: 1,
      giveUpMs: 10,
    })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))
    await router.send(thread.id, "alice", "hello")
    await until(() => seen.some(e => e.type === "notice" && e.text.includes("cannot reach")))
    await new Promise(r => setTimeout(r, 30))
    expect(store.get(thread.id)?.state).toBe("running")
    expect(calls.some(c => c.startsWith("end-turn"))).toBe(false)
  })

  it("a restarted runner (new generation) is read again from its first event", async () => {
    const { dp, setGeneration, setEvents } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    const seen: UiEvent[] = []
    router.subscribe(thread.id, e => seen.push(e))
    await router.send(thread.id, "alice", "one")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(store.get(thread.id)?.cursor).toBe(3)

    // The runner restarted: numbering starts at 1 and the new turn has two events.
    setGeneration("gen-2")
    setEvents([
      { seq: 1, kind: "agent", event: { type: "text-after-restart" } },
      { seq: 2, kind: "turn-end", exitCode: 0 },
    ])
    const turns = seen.filter(e => e.type === "agent").length
    await router.send(thread.id, "alice", "two")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(seen.filter(e => e.type === "agent").length).toBe(turns + 1)
    expect(store.get(thread.id)?.cursor).toBe(2)
    expect(store.get(thread.id)?.generation).toBe("gen-2")
  })

  it("after a restart, a thread saved as running is read again and finished; idle ones get timers", async () => {
    const { dp, calls } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    store.put({
      id: "r1",
      title: "a",
      user: "alice",
      owner: "alice",
      sandboxId: "sbx-9",
      state: "running",
      lastActivity: 1,
    })
    store.put({
      id: "i1",
      title: "b",
      user: "alice",
      owner: "alice",
      sandboxId: "sbx-8",
      state: "idle",
      lastActivity: 1,
    })
    const router = new ThreadRouter(dp, store, { idlePauseMs: 10, pollMs: 1 })
    router.recover()
    // The runner says no turn is running and has no events: the thread is finished.
    await until(() => store.get("r1")?.state !== "running")
    expect(calls).toContain("end-turn r1 sbx-9")
    await until(() => calls.includes("pause sbx-8 end"))
  })

  it("tells listeners each event's place in the log", async () => {
    const { dp } = fakeDataPlane({ pauseResume: true })
    const store = new ThreadStore()
    const router = new ThreadRouter(dp, store, { idlePauseMs: 60_000, pollMs: 1 })
    const thread = router.create("Orders", "alice")
    const seen: number[] = []
    router.subscribe(thread.id, (_event, index) => seen.push(index))
    await router.send(thread.id, "alice", "hello")
    await until(() => store.get(thread.id)?.state === "idle")
    expect(seen).toEqual(seen.map((_, i) => i))
    expect(seen.length).toBe(router.history(thread.id).length)
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
