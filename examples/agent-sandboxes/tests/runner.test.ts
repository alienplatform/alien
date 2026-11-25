import { spawnSync } from "node:child_process"
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import { SWEEP_SCRIPT, VERIFY_SCRIPT } from "../data-plane/runner/src/processes"
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

// The sweep is `kill -9 -1` and the verify lists what is left. Most of these tests run both in a
// throwaway PID namespace of their own (the namespace's PID 1 is spared by `kill -1`, so the
// outer shell survives); where one cannot be made, or perl is missing, they are skipped. The
// verify's parsing tests use a fake /proc directory and run anywhere.
const ENV = { PATH: process.env.PATH, SWEEP: SWEEP_SCRIPT, VERIFY: VERIFY_SCRIPT }
const nsArgs = (mapping: string) => ["--user", mapping, "--pid", "--fork", "--mount-proc"]
const inNamespace = (mapping: string, script: string) =>
  spawnSync("unshare", [...nsArgs(mapping), "sh", "-c", script, "outer"], {
    encoding: "utf8",
    env: ENV,
    timeout: 30000,
  })
const canNamespace = (mapping: string) =>
  spawnSync("unshare", [...nsArgs(mapping), "true"]).status === 0 &&
  spawnSync("perl", ["-e", "1"]).status === 0

describe("ending what a thread left running", () => {
  const ok = canNamespace("--map-user=1000")

  it.skipIf(!ok)(
    "ends a re-forking loop, a process with a blanked command line and a child of a long-lived parent",
    () => {
      const result = inNamespace(
        "--map-user=1000",
        [
          // A process that blanks its own argv (the old sweep skipped those as kernel threads).
          "perl -e '$0=\"\"; sleep 600' &",
          // A loop that forks detached children as fast as it can (a /proc walk cannot catch it).
          "sh -c 'while :; do (sleep 500 &); sleep 0.01; done' &",
          // A long-lived parent with an old child under it.
          "setsid sh -c '(sleep 700 &); sleep 701' &",
          "sleep 0.5",
          'sh -c "$VERIFY" verify > /tmp/before.$$; echo "before: $?"',
          'sh -c "$SWEEP" sweep; echo "sweep done"',
          'sh -c "$VERIFY" verify > /tmp/after.$$; echo "after: $? left: $(wc -l < /tmp/after.$$)"',
          "rm -f /tmp/before.$$ /tmp/after.$$",
        ].join("\n"),
      )
      expect(result.stdout).toContain("before: 1")
      expect(result.stdout).toContain("after: 0 left: 0")
    },
    40000,
  )

  it.skipIf(!ok)("the verify fails on a leftover and names it", () => {
    const result = inNamespace(
      "--map-user=1000",
      'sleep 400 & sleep 0.2; sh -c "$VERIFY" verify > /tmp/o.$$; echo "exit: $?"; cat /tmp/o.$$ | tr "\\n" " "; echo; rm -f /tmp/o.$$',
    )
    expect(result.stdout).toContain("exit: 1")
  })
})

describe("the verify's reading of /proc", () => {
  const uid = process.getuid?.() ?? 1000
  const fake = (entries: Record<string, { status?: string; stat?: string }>) => {
    const dir = mkdtempSync(join(tmpdir(), "fakeproc-"))
    for (const [pid, files] of Object.entries(entries)) {
      mkdirSync(join(dir, pid))
      if (files.status !== undefined) writeFileSync(join(dir, pid, "status"), files.status)
      if (files.stat !== undefined) writeFileSync(join(dir, pid, "stat"), files.stat)
    }
    return dir
  }
  const verify = (dir: string) =>
    spawnSync("sh", ["-c", VERIFY_SCRIPT, "verify"], {
      encoding: "utf8",
      env: { PATH: process.env.PATH, PROC_DIR: dir },
    })
  const status = (u: number) => `Name:\tx\nUid:\t${u}\t${u}\t${u}\t${u}\n`
  // The name is field 2 and may hold spaces and parentheses: cut after the last ")".
  const statLine = (state: string) => `4242 (we ) ird name) ${state} 1 4242 4242 0 -1\n`

  it("lists a running process of the sandbox user and exits 1", () => {
    const r = verify(fake({ "4242": { status: status(uid), stat: statLine("S") } }))
    expect(r.status).toBe(1)
    expect(r.stdout.trim()).toBe("4242")
  })
  it("skips zombies, other users' processes", () => {
    const r = verify(
      fake({
        "4242": { status: status(uid), stat: statLine("Z") },
        "4243": { status: status(uid + 1), stat: statLine("S") },
      }),
    )
    expect(r.status).toBe(0)
    expect(r.stdout).toBe("")
  })
  it("fails closed when a process of the sandbox user cannot be read", () => {
    const r = verify(fake({ "4242": { status: status(uid) } }))
    expect(r.status).toBe(2)
    expect(r.stderr).toContain("cannot read")
    expect(verify(fake({ "4242": {} })).status).toBe(2)
  })
})

describe("the sweep and the verify as root", () => {
  const ok = canNamespace("--map-root-user")
  it.skipIf(!ok)("both refuse, and the sweep ends nothing", () => {
    const result = inNamespace(
      "--map-root-user",
      'sleep 500 & sleep 0.2; sh -c "$SWEEP" sweep; echo "sweep exit: $?"; sh -c "$VERIFY" verify; echo "verify exit: $?"; kill -0 $! && echo "sleep alive"',
    )
    expect(result.stderr).toContain("the sweep must run as the sandbox user, not root")
    expect(result.stderr).toContain("the verify must run as the sandbox user, not root")
    expect(result.stdout).toContain("sweep exit: 1")
    expect(result.stdout).toContain("verify exit: 1")
    expect(result.stdout).toContain("sleep alive")
  })
})

describe("the sweep and process owners", () => {
  // The runtime starts its own helpers as root; the sweep runs as the sandbox user and ends only
  // that user's processes. Needs root to make both kinds, so it is skipped without it.
  const root = spawnSync("sudo", ["-n", "unshare", "--pid", "--fork", "--mount-proc", "true"])
  it.skipIf(root.status !== 0)("leaves processes of another user alone", () => {
    const as = "setpriv --reuid=65534 --regid=65534 --clear-groups"
    const result = spawnSync(
      "sudo",
      [
        "-n",
        "env",
        `SWEEP=${SWEEP_SCRIPT}`,
        `VERIFY=${VERIFY_SCRIPT}`,
        "unshare",
        "--pid",
        "--fork",
        "--mount-proc",
        "sh",
        "-c",
        [
          "sleep 400 &",
          `${as} sleep 401 &`,
          "sleep 0.3",
          `${as} sh -c "$SWEEP" sweep`,
          `${as} sh -c "$VERIFY" verify > /tmp/left.$$; echo "left: $? $(wc -l < /tmp/left.$$)"`,
          "rm -f /tmp/left.$$",
          // the root-owned sleep is still there
          'for p in /proc/[0-9]*; do tr "\\0" " " < $p/cmdline; echo; done | grep -c "^sleep 400" | sed "s/^/root sleep alive: /"',
        ].join("\n"),
        "outer",
      ],
      { encoding: "utf8", env: { PATH: process.env.PATH } },
    )
    expect(result.stdout).toContain("left: 0 0")
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
