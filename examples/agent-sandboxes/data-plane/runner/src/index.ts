// Runner: holds the sandbox binding and runs the agent. The control plane drives it with
// commands; nothing here is reachable from a sandbox.
//
//   open       { threadId, sandboxId? }             create the thread's sandbox, or resume it
//   bind-turn  { sandboxId, token }                 install the turn token on the connector link
//   turn       { threadId, sandboxId, sessionId?, user, text }   start one agent turn
//   events     { threadId, after, waitMs? }         long-poll the turn's events
//   exec       { sandboxId, command }               run a shell command in the sandbox
//   pause      { threadId, sandboxId }              pause an idle sandbox
//   close      { sandboxId }                        end it
//
// One turn is one process: `claude -p` with stream-json in and out, `--resume <session>` after
// the first turn. Claude Code keeps the conversation on the sandbox's disk, so nothing runs
// between turns and a pause never cuts a live stream.

import { randomUUID } from "node:crypto"
import { sandbox } from "@alienplatform/bindings"
import { createCommandReceiver } from "@alienplatform/commands"
import { LineSplitter, ThreadEvents } from "./turns"

const agent = sandbox("agent")
const MODEL = process.env.MODEL ?? "claude-sonnet-4-6"
const TURN_TIMEOUT_MS = 30 * 60 * 1000
// A held write waits for approval inside one Bash call, so Bash may wait as long as approvals do.
const BASH_TIMEOUT_MS = String((Number(process.env.APPROVAL_TIMEOUT_SECONDS ?? 600) + 60) * 1000)

// What every sandbox starts with: placeholders only. The real values never enter the sandbox:
// the model container holds the cloud identity, and the gateway adds the turn token on the link.
const PLACEHOLDER_ENV = {
  ANTHROPIC_API_KEY: "placeholder",
  CONNECTOR_TOKEN: "placeholder",
}

const threads = new Map<string, ThreadEvents>()
function thread(threadId: string): ThreadEvents {
  let t = threads.get(threadId)
  if (!t) {
    t = new ThreadEvents()
    threads.set(threadId, t)
  }
  return t
}

function obj(input: unknown): Record<string, unknown> {
  if (typeof input !== "object" || input === null) throw new TypeError("input must be an object")
  return input as Record<string, unknown>
}
function str(input: Record<string, unknown>, name: string): string {
  const value = input[name]
  if (typeof value !== "string" || value === "") throw new TypeError(`${name} must be a non-empty string`)
  return value
}
function optStr(input: Record<string, unknown>, name: string): string | undefined {
  const value = input[name]
  return typeof value === "string" && value !== "" ? value : undefined
}

// The image's root is read-only; a sandbox writes under /sandbox, the same on every platform.
const WORKSPACE = "/sandbox/workspace"
const HOME_DIR = "/sandbox/home"
const TMP_DIR = "/sandbox/tmp"
// A directory argument resolves against /sandbox, so the same place by its relative name.
const WORKSPACE_DIR = "workspace"

// Gives a new sandbox its working directory and its instructions file.
async function prepareWorkspace(sandboxId: string) {
  for await (const frame of agent.runCommand(sandboxId, "sh", {
    args: ["-c", `mkdir -p ${WORKSPACE} ${HOME_DIR} ${TMP_DIR} && cp /opt/agent/CLAUDE.md ${WORKSPACE}/CLAUDE.md`],
    timeoutMs: 30_000,
  })) {
    if (frame.kind === "exit" && frame.exitCode !== 0) throw new Error("preparing the workspace failed")
  }
}

async function agentEnv(): Promise<Record<string, string>> {
  const modelUrl = await agent.linkUrl("model", 8080)
  const connectorUrl = await agent.linkUrl("connector", 8080)
  if (!modelUrl || !connectorUrl) throw new Error("the agent sandbox must link model and connector")
  return {
    // Also passed with every command: a local sandbox takes its environment from the command.
    ...PLACEHOLDER_ENV,
    ANTHROPIC_BASE_URL: modelUrl.replace(/\/$/, ""),
    CONNECTOR_URL: connectorUrl.replace(/\/$/, ""),
    ANTHROPIC_MODEL: MODEL,
    ANTHROPIC_SMALL_FAST_MODEL: MODEL,
    ANTHROPIC_DEFAULT_HAIKU_MODEL: MODEL,
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    DISABLE_AUTOUPDATER: "1",
    HOME: HOME_DIR,
    TMPDIR: TMP_DIR,
    // Node's fetch (the `orders` command) sends through the sandbox's proxy only when asked.
    NODE_USE_ENV_PROXY: "1",
    NODE_NO_WARNINGS: "1",
    CLAUDE_CODE_TMPDIR: TMP_DIR,
    BASH_DEFAULT_TIMEOUT_MS: BASH_TIMEOUT_MS,
    BASH_MAX_TIMEOUT_MS: BASH_TIMEOUT_MS,
  }
}

async function runTurn(t: ThreadEvents, sandboxId: string, sessionId: string | undefined, user: string, text: string) {
  const turnId = randomUUID()
  t.running = true
  t.push({ kind: "turn-start", turnId, user })
  const args = [
    "-p",
    "--input-format", "stream-json",
    "--output-format", "stream-json",
    "--verbose",
    "--model", MODEL,
    "--permission-mode", "acceptEdits",
    "--allowedTools", "Bash,Read,Write,Edit,Glob,Grep",
    ...(sessionId ? ["--resume", sessionId] : []),
  ]
  let exitCode: number | null = null
  let error: string | undefined
  try {
    const proc = await agent.runInteractive(sandboxId, "claude", {
      args,
      cwd: WORKSPACE_DIR,
      env: await agentEnv(),
      timeoutMs: TURN_TIMEOUT_MS,
    })
    const message = { type: "user", message: { role: "user", content: [{ type: "text", text }] } }
    await proc.write(`${JSON.stringify(message)}\n`)
    // End of input: Claude Code answers the one message, prints its `result` and exits.
    await proc.closeStdin()
    const out = new LineSplitter()
    const err = new LineSplitter()
    const onLine = (line: string) => {
      try {
        const event = JSON.parse(line) as Record<string, unknown>
        if (event.type === "system" && event.subtype === "init" && typeof event.session_id === "string") {
          t.sessionId = event.session_id
        }
        t.push({ kind: "agent", event })
      } catch {
        t.push({ kind: "stderr", text: line })
      }
    }
    for await (const frame of proc) {
      if (frame.kind === "stdout") out.feed(frame.data.toString("utf8")).forEach(onLine)
      else if (frame.kind === "stderr") err.feed(frame.data.toString("utf8")).forEach(text => t.push({ kind: "stderr", text }))
      else if (frame.kind === "exit") exitCode = frame.exitCode
    }
    out.flush().forEach(onLine)
  } catch (e) {
    error = e instanceof Error ? e.message : String(e)
    console.error(`turn ${turnId} failed`, e)
  } finally {
    t.running = false
    t.push({ kind: "turn-end", turnId, exitCode, ...(error ? { error } : {}) })
  }
}

// Several commands at once (an events long-poll must not hold up an approval), polled often
// enough for a chat.
const receiver = createCommandReceiver({ maxLeases: 16, pollIntervalMs: 1000 })

receiver.command("open", async input => {
  const i = obj(input)
  const threadId = str(i, "threadId")
  const known = optStr(i, "sandboxId")
  if (known) {
    const found = await agent.get(known)
    if (found && found.state === "paused") {
      await agent.resume(known)
      return { sandboxId: known, created: false, resumed: true }
    }
    if (found && found.state !== "terminated") return { sandboxId: known, created: false, resumed: false }
  }
  const created = await agent.create({ env: PLACEHOLDER_ENV })
  await prepareWorkspace(created.sandboxId)
  // A fresh sandbox starts a fresh conversation.
  thread(threadId).sessionId = undefined
  console.log(`thread ${threadId}: sandbox ${created.sandboxId}`)
  return { sandboxId: created.sandboxId, created: true, resumed: false }
})

// A sandbox credential rule: the gateway sets this header on the sandbox's
// plain-HTTP requests to the link, and the call returns once every gateway replica has it.
interface CredentialRule {
  link: string
  header: string
  valuePrefix?: string
  value: string
}
interface SandboxCredentials {
  updateCredentials(sandboxId: string, rules: CredentialRule[]): Promise<void>
}
function hasCredentials(s: object): s is SandboxCredentials {
  return "updateCredentials" in s && typeof s.updateCredentials === "function"
}

receiver.command("bind-turn", async input => {
  const i = obj(input)
  const sandboxId = str(i, "sandboxId")
  const token = str(i, "token")
  if (!hasCredentials(agent)) {
    return { bound: false, reason: "this SDK has no sandbox credentials API yet" }
  }
  await agent.updateCredentials(sandboxId, [
    { link: "connector", header: "Authorization", valuePrefix: "Bearer ", value: token },
  ])
  return { bound: true }
})

receiver.command("turn", async input => {
  const i = obj(input)
  const threadId = str(i, "threadId")
  const sandboxId = str(i, "sandboxId")
  const user = str(i, "user")
  const text = str(i, "text")
  const t = thread(threadId)
  if (t.running) throw new Error("a turn is already running in this thread")
  // The router's store survives a runner restart; this process's memory may not.
  const sessionId = optStr(i, "sessionId") ?? t.sessionId
  void runTurn(t, sandboxId, sessionId, user, text)
  return { started: true }
})

receiver.command("events", async input => {
  const i = obj(input)
  const t = thread(str(i, "threadId"))
  const after = typeof i.after === "number" ? i.after : 0
  const waitMs = Math.min(typeof i.waitMs === "number" ? i.waitMs : 15_000, 20_000)
  const events = await t.after(after, waitMs)
  return { events, running: t.running, sessionId: t.sessionId ?? null }
})

receiver.command("exec", async input => {
  const i = obj(input)
  const sandboxId = str(i, "sandboxId")
  const command = str(i, "command")
  let stdout = ""
  let stderr = ""
  let exitCode: number | null = null
  for await (const frame of agent.runCommand(sandboxId, "sh", {
    args: ["-c", command],
    cwd: WORKSPACE_DIR,
    env: await agentEnv(),
    timeoutMs: 30_000,
  })) {
    if (frame.kind === "stdout") stdout += frame.data.toString("utf8")
    else if (frame.kind === "stderr") stderr += frame.data.toString("utf8")
    else if (frame.kind === "exit") exitCode = frame.exitCode
  }
  return { exitCode, stdout: stdout.slice(-20_000), stderr: stderr.slice(-20_000) }
})

receiver.command("pause", async input => {
  const i = obj(input)
  if (thread(str(i, "threadId")).running) throw new Error("a turn is running; pause after it ends")
  const sandboxId = str(i, "sandboxId")
  if (!(await agent.capabilities(sandboxId)).includes("pauseResume")) {
    return { paused: false, reason: "this platform keeps sandboxes running instead of pausing them" }
  }
  await agent.pause(sandboxId)
  return { paused: true }
})

receiver.command("close", async input => {
  await agent.terminate(str(obj(input), "sandboxId"))
  return { closed: true }
})

console.log("runner leasing commands")

async function main(): Promise<void> {
  await receiver.run()
}

void main()
