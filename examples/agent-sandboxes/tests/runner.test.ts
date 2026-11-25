import { spawnSync } from "node:child_process"
import { describe, expect, it } from "vitest"
import { SWEEP_SCRIPT } from "../data-plane/runner/src/processes"
import {
  LineSplitter,
  pauseWhenIdle,
  startTurn,
  stopTurn,
  ThreadEvents,
} from "../data-plane/runner/src/turns"

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
    const turn = startTurn(t, "turn-x", () => order.push("turn start"))
    await Promise.all([pausing, turn])
    expect(order).toEqual(["pause start", "pause end", "turn start"])
    expect(t.running).toBe(true)
  })

  it("a turn claimed between the capability check and the pause stops the pause", async () => {
    const t = new ThreadEvents()
    let paused = false
    const turn = startTurn(t, "turn-x", () => {})
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
    await startTurn(t, "turn-x", () => {})
    await expect(startTurn(t, "turn-x", () => {})).rejects.toThrow("already running")
  })

  it("a stop that names another turn leaves the running turn alone", async () => {
    const t = new ThreadEvents()
    let killed = 0
    await startTurn(t, "turn-2", () => {})
    t.stop = async () => {
      killed += 1
    }
    expect(await stopTurn(t, "turn-1")).toEqual({ stopped: false, running: false })
    expect(killed).toBe(0)
    expect((await stopTurn(t, "turn-2")).stopped).toBe(true)
    expect(killed).toBe(1)
  })

  it("a stop after the turn ended does nothing", async () => {
    const t = new ThreadEvents()
    expect(await stopTurn(t, "turn-1")).toEqual({ stopped: false, running: false })
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

describe("ending what a thread left running", () => {
  // The sweep ends everything but the sandbox's own processes, so the test runs it in a
  // throwaway PID namespace (as PID 1 there is the "sandbox's own" process); where such a
  // namespace cannot be made the test is skipped.
  const unshare = spawnSync("unshare", [
    "--user",
    "--map-user=1000",
    "--pid",
    "--fork",
    "--mount-proc",
    "true",
  ])
  const run = (script: string) =>
    spawnSync(
      "unshare",
      ["--user", "--map-user=1000", "--pid", "--fork", "--mount-proc", "sh", "-c", script, "outer"],
      { encoding: "utf8", env: { PATH: process.env.PATH, SWEEP: SWEEP_SCRIPT } },
    )

  it.skipIf(unshare.status !== 0)(
    "ends every process, whatever env or name it has, then lists none",
    () => {
      const result = run(
        [
          // Processes the agent might leave: one that renames itself through env, a daemon that
          // detaches, and a plain background job.
          "SANDBOX_TURN_USER=nobody sleep 300 &",
          "(setsid sleep 301 &) ",
          "sleep 302 &",
          "sleep 0.3",
          // No pipes: a pipe's other end would be a sibling of the sweep, and be swept too.
          'sh -c "$SWEEP" sweep list > /tmp/before.$$; echo "before: $(wc -l < /tmp/before.$$)"',
          'sh -c "$SWEEP" sweep sweep > /tmp/ended.$$; echo "ended: $(wc -l < /tmp/ended.$$)"',
          'sh -c "$SWEEP" sweep list > /tmp/after.$$; echo "after: $(wc -l < /tmp/after.$$)"',
          "rm -f /tmp/before.$$ /tmp/ended.$$ /tmp/after.$$",
        ].join("\n"),
      )
      expect(result.stdout).toContain("ended: 3")
      expect(result.stdout).toContain("after: 0")
      expect(result.stdout).not.toContain("before: 0")
    },
  )
})

describe("the sweep's own command wrapper", () => {
  const ns = spawnSync("unshare", [
    "--user",
    "--map-user=1000",
    "--pid",
    "--fork",
    "--mount-proc",
    "true",
  ])
  // The runtime starts every command in a shell with a watchdog subshell (and its sleep) beside
  // the command; those belong to the sweep's own command, so the listing afterwards is empty.
  it.skipIf(ns.status !== 0)("is not counted as left over", () => {
    const result = spawnSync(
      "unshare",
      [
        "--user",
        "--map-user=1000",
        "--pid",
        "--fork",
        "--mount-proc",
        "sh",
        "-c",
        'sleep 600 & sleep 0.2; sh -c \'( sleep 31 & wait ) & sh -c "$SWEEP" sweep sweep > /tmp/e.$$; sh -c "$SWEEP" sweep list > /tmp/l.$$; echo "ended: $(wc -l < /tmp/e.$$)"; echo "left: $(wc -l < /tmp/l.$$)"; rm -f /tmp/e.$$ /tmp/l.$$; kill $(jobs -p) 2>/dev/null\'',
        "outer",
      ],
      { encoding: "utf8", env: { PATH: process.env.PATH, SWEEP: SWEEP_SCRIPT } },
    )
    expect(result.stdout).toContain("ended: 1")
    expect(result.stdout).toContain("left: 0")
  })
})

describe("the sweep as root", () => {
  const ns = spawnSync("unshare", [
    "--user",
    "--map-root-user",
    "--pid",
    "--fork",
    "--mount-proc",
    "true",
  ])
  it.skipIf(ns.status !== 0)("refuses, ends nothing and exits non-zero", () => {
    const result = spawnSync(
      "unshare",
      [
        "--user",
        "--map-root-user",
        "--pid",
        "--fork",
        "--mount-proc",
        "sh",
        "-c",
        'sleep 500 & sleep 0.2; sh -c "$SWEEP" sweep sweep; echo "exit: $?"; kill -0 $! && echo "sleep alive"',
        "outer",
      ],
      { encoding: "utf8", env: { PATH: process.env.PATH, SWEEP: SWEEP_SCRIPT } },
    )
    expect(result.stderr).toContain("the sweep must run as the sandbox user, not root")
    expect(result.stdout).toContain("exit: 1")
    expect(result.stdout).toContain("sleep alive")
  })
})

describe("the sweep and process owners", () => {
  // The runtime starts its own helpers as root; the sweep runs as the sandbox user and may only
  // end that user's processes. Needs root to make both kinds, so it is skipped without it.
  const root = spawnSync("sudo", ["-n", "unshare", "--pid", "--fork", "--mount-proc", "true"])
  it.skipIf(root.status !== 0)("leaves processes of another user alone", () => {
    const result = spawnSync(
      "sudo",
      [
        "-n",
        "env",
        `SWEEP=${SWEEP_SCRIPT}`,
        "unshare",
        "--pid",
        "--fork",
        "--mount-proc",
        "sh",
        "-c",
        [
          "sleep 400 &",
          "setpriv --reuid=65534 --regid=65534 --clear-groups sleep 401 &",
          "sleep 0.3",
          'setpriv --reuid=65534 --regid=65534 --clear-groups sh -c "$SWEEP" sweep sweep > /tmp/ended.$$; echo "ended: $(wc -l < /tmp/ended.$$)"',
          'setpriv --reuid=65534 --regid=65534 --clear-groups sh -c "$SWEEP" sweep list > /tmp/left.$$; echo "left: $(wc -l < /tmp/left.$$)"',
          "rm -f /tmp/ended.$$ /tmp/left.$$",
          // the root-owned sleep is still there
          'for p in /proc/[0-9]*; do tr "\\0" " " < $p/cmdline; echo; done | grep -c "^sleep 400" | sed "s/^/root sleep alive: /"',
        ].join("\n"),
        "outer",
      ],
      { encoding: "utf8", env: { PATH: process.env.PATH, SWEEP: SWEEP_SCRIPT } },
    )
    expect(result.stdout).toContain("ended: 1")
    expect(result.stdout).toContain("left: 0")
    expect(result.stdout).toContain("root sleep alive: 1")
  })
})

describe("reading the agent's output", () => {
  it("a character split across two frames arrives whole", () => {
    const bytes = Buffer.from('{"text":"héllo 日本"}\n')
    const cut = bytes.indexOf(0xc3) + 1 // between the two bytes of "é"
    const splitter = new LineSplitter()
    const lines = [
      ...splitter.feedBytes(bytes.subarray(0, cut)),
      ...splitter.feedBytes(bytes.subarray(cut)),
    ]
    expect(lines).toEqual(['{"text":"héllo 日本"}'])
    const second = Buffer.from("日本\n")
    const lines2 = [
      ...splitter.feedBytes(second.subarray(0, 2)),
      ...splitter.feedBytes(second.subarray(2)),
    ]
    expect(lines2).toEqual(["日本"])
  })
})
