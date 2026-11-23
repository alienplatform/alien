import { spawn } from "node:child_process"
import { describe, expect, it } from "vitest"
import { END_PROCESSES_SCRIPT, TURN_USER_VAR } from "../data-plane/runner/src/processes"
import { pauseWhenIdle, startTurn, ThreadEvents } from "../data-plane/runner/src/turns"

const wait = (ms: number) => new Promise(r => setTimeout(r, ms))

describe("a thread's lock in the runner", () => {
  it("a turn that arrives during a pause starts after the pause, not inside it", async () => {
    const t = new ThreadEvents()
    const order: string[] = []
    const pausing = pauseWhenIdle(
      t,
      async () => true,
      async () => {
        order.push("pause start")
        await wait(30)
        order.push("pause end")
      },
    )
    await wait(5)
    const turn = startTurn(t, () => order.push("turn start"))
    await Promise.all([pausing, turn])
    expect(order).toEqual(["pause start", "pause end", "turn start"])
    expect(t.running).toBe(true)
  })

  it("a turn claimed between the capability check and the pause stops the pause", async () => {
    const t = new ThreadEvents()
    let paused = false
    const turn = startTurn(t, () => {})
    const pausing = pauseWhenIdle(
      t,
      async () => true,
      async () => {
        paused = true
      },
    )
    await turn
    await expect(pausing).rejects.toThrow("a turn is running")
    expect(paused).toBe(false)
  })

  it("refuses a second turn while one runs", async () => {
    const t = new ThreadEvents()
    await startTurn(t, () => {})
    await expect(startTurn(t, () => {})).rejects.toThrow("already running")
  })

  it("a platform without pause reports it and pauses nothing", async () => {
    const t = new ThreadEvents()
    let paused = false
    const result = await pauseWhenIdle(
      t,
      async () => false,
      async () => {
        paused = true
      },
    )
    expect(result.paused).toBe(false)
    expect(paused).toBe(false)
  })
})

describe("ending a user's processes", () => {
  // The same script the runner runs in the sandbox, here against real processes.
  const start = (user: string | undefined) =>
    spawn("sleep", ["30"], { env: { PATH: process.env.PATH, ...(user ? { [TURN_USER_VAR]: user } : {}) } })
  const alive = (pid: number | undefined) => {
    try {
      process.kill(pid as number, 0)
      return true
    } catch {
      return false
    }
  }

  it("ends the previous user's processes and leaves everyone else's", async () => {
    const alice = start("alice")
    const bob = start("bob")
    const bystander = start(undefined)
    const script = spawn("sh", ["-c", END_PROCESSES_SCRIPT], { env: { PATH: process.env.PATH, OWNER: "alice" } })
    let out = ""
    script.stdout.on("data", d => (out += d))
    await new Promise(resolve => script.on("close", resolve))
    await wait(50)
    try {
      expect(out.split("\n").filter(Boolean)).toContain(String(alice.pid))
      expect(alive(alice.pid) && !alice.killed && alice.exitCode === null && alice.signalCode === null).toBe(false)
      expect(bob.exitCode === null && bob.signalCode === null).toBe(true)
      expect(bystander.exitCode === null && bystander.signalCode === null).toBe(true)
    } finally {
      bob.kill()
      bystander.kill()
      alice.kill()
    }
  })
})
