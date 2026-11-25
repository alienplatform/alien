// Thread router: one sandbox per thread.
//
// A message opens the thread's sandbox (creating it, or resuming it if it was paused), binds it
// to the user who is asking, and only then starts the turn. While the turn runs, the router
// long-polls the runner for the agent's events and the connector for writes waiting for
// approval, and fans both out to the browser. When the turn ends its access ends with it. A
// thread left idle is paused.
//
// Opening, starting a turn, pausing and resuming a thread's sandbox run one at a time (one lock
// per thread), so a thread gets one sandbox and a pause cannot catch a message half way.

import { bindTurn } from "./bind-turn"
import type { DataPlane, Decided, EventsResult, PendingWrite } from "./data-plane"
import { KeyedLock } from "./lock"
import type { Thread, ThreadStore } from "./store"

/** The thread is still answering; the caller may retry. */
export class BusyError extends Error {}

export type UiEvent =
  | { type: "user"; user: string; text: string }
  | { type: "agent"; event: Record<string, unknown> }
  | { type: "stderr"; text: string }
  | { type: "notice"; text: string }
  | { type: "state"; state: Thread["state"] }
  | { type: "approvals"; pending: PendingWrite[] }
  | { type: "exec"; command: string; exitCode: number | null; stdout: string; stderr: string }

/** `index` is the event's place in the thread's log, so a reader can skip what it already has. */
type Listener = (event: UiEvent, index: number) => void

export class ThreadRouter {
  private logs = new Map<string, UiEvent[]>()
  private listeners = new Map<string, Set<Listener>>()
  private idleTimers = new Map<string, ReturnType<typeof setTimeout>>()
  private idleGen = new Map<string, number>()
  private execs = new Map<string, number>()
  private pumping = new Set<string>()
  private locks = new KeyedLock()

  constructor(
    private readonly dp: DataPlane,
    private readonly store: ThreadStore,
    private readonly options: {
      idlePauseMs: number
      pollMs?: number
      /** First wait after a failed poll; doubles up to 16 times this. */
      retryMs?: number
      /** How long polls may keep failing before the turn is stopped through the runner. */
      giveUpMs?: number
    },
  ) {}

  create(title: string, user: string): Thread {
    const id = `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`
    return this.store.put({ id, title, user, state: "new", lastActivity: Date.now() })
  }

  history(threadId: string): UiEvent[] {
    return this.logs.get(threadId) ?? []
  }

  subscribe(threadId: string, listener: Listener): () => void {
    const set = this.listeners.get(threadId) ?? new Set()
    set.add(listener)
    this.listeners.set(threadId, set)
    return () => set.delete(listener)
  }

  private emit(threadId: string, event: UiEvent): void {
    const log = this.logs.get(threadId) ?? []
    const index = log.length
    log.push(event)
    this.logs.set(threadId, log)
    for (const listener of this.listeners.get(threadId) ?? []) listener(event, index)
  }

  private setState(threadId: string, state: Thread["state"]): Thread {
    const thread = this.store.update(threadId, { state, lastActivity: Date.now() })
    this.emit(threadId, { type: "state", state })
    return thread
  }

  /** Opens the thread's sandbox, resuming it if it was paused. */
  private async open(thread: Thread): Promise<Thread> {
    const opened = await this.dp.open(thread.id, thread.sandboxId)
    if (opened.resumed)
      this.emit(thread.id, { type: "notice", text: `resumed sandbox ${opened.sandboxId}` })
    if (opened.created) {
      this.emit(thread.id, { type: "notice", text: `created sandbox ${opened.sandboxId}` })
      return this.store.update(thread.id, { sandboxId: opened.sandboxId, sessionId: undefined })
    }
    return this.store.update(thread.id, { sandboxId: opened.sandboxId })
  }

  /** Starts a turn. Resolves once the turn has started; its events arrive through `subscribe`. */
  async send(threadId: string, user: string, text: string): Promise<void> {
    await this.locks.run(threadId, async () => {
      let thread = this.store.get(threadId)
      if (!thread) throw new Error(`no thread ${threadId}`)
      if (thread.state === "running") throw new BusyError("this thread is already answering")
      this.clearIdle(threadId)
      // Whose processes may be in the sandbox: the owner, which only a finished sweep and bind
      // change. Threads saved before `owner` existed fall back to the last asking user.
      const owner = thread.owner ?? thread.user
      if (thread.user !== user)
        this.emit(threadId, { type: "notice", text: `user switched to ${user}` })
      // A thread saved before `owner` existed has none: the previous user is the owner until a
      // sweep and bind for the new user succeed, so a failed first sweep is tried again.
      this.store.update(threadId, { user, ...(thread.owner === undefined ? { owner } : {}) })
      this.emit(threadId, { type: "user", user, text })
      this.setState(threadId, "running")
      try {
        const hadSandbox = thread.sandboxId !== undefined
        thread = await this.open(thread)
        const sandboxId = thread.sandboxId as string
        // Nothing the previous owner left running may run as the new user. The data plane ends
        // everything that is not the sandbox's own and checks the list is empty; if it cannot,
        // the turn does not start.
        if (owner !== user && hadSandbox) {
          await this.dp.endProcesses(sandboxId)
          this.emit(threadId, {
            type: "notice",
            text: `ended the processes that ${owner}'s turns left running`,
          })
        }
        // The turn does not start until the gateway has the new user's token.
        await bindTurn(this.dp, { threadId, sandboxId }, user)
        this.emit(threadId, { type: "notice", text: `sandbox bound to ${user}` })
        this.store.update(threadId, { owner: user })
        const started = await this.dp.turn({
          threadId,
          sandboxId,
          sessionId: thread.sessionId,
          text,
        })
        this.store.update(threadId, { turnId: started.turnId })
      } catch (error) {
        this.emit(threadId, { type: "notice", text: `turn failed to start: ${message(error)}` })
        await this.finish(threadId)
        throw error
      }
    })
    void this.pump(threadId)
  }

  /** Reattaches to the turns that were running when the control plane stopped, and re-arms idle timers. */
  recover(): void {
    for (const thread of this.store.list()) {
      if (thread.state === "running") {
        this.emit(thread.id, {
          type: "notice",
          text: "the control plane restarted; reading the turn's events again",
        })
        void this.pump(thread.id)
      } else if (thread.state === "idle") this.armIdle(thread.id)
    }
  }

  /**
   * Reads the turn's events until the turn ends. A failed poll is retried with a growing wait:
   * the turn may well still be running, so the thread is not finished (and its access not
   * ended) on a poll error. After `giveUpMs` of failures the turn is stopped through the
   * runner; if even that fails the thread stays `running` and the polling goes on.
   */
  private async pump(threadId: string): Promise<void> {
    if (this.pumping.has(threadId)) return
    this.pumping.add(threadId)
    const pollMs = this.options.pollMs ?? 2000
    const retryMs = this.options.retryMs ?? 1000
    const giveUpMs = this.options.giveUpMs ?? 120_000
    let pending = ""
    let failures = 0
    let failingSince = 0
    let warned = false
    for (;;) {
      const known = this.store.get(threadId)
      if (!known) break
      let result: EventsResult
      try {
        const [events, writes] = await Promise.all([
          this.dp.events(threadId, known.cursor ?? 0, pollMs),
          // A connector that is briefly away does not end the turn.
          this.dp.approvals(threadId).catch(() => undefined),
        ])
        result = events
        if (writes) {
          const key = writes.map(w => w.id).join(",")
          if (key !== pending) {
            pending = key
            this.emit(threadId, { type: "approvals", pending: writes })
          }
        }
      } catch (error) {
        if (failures === 0) failingSince = Date.now()
        failures += 1
        if (Date.now() - failingSince >= giveUpMs) {
          try {
            const turnId = this.store.get(threadId)?.turnId
            if (!turnId) throw new Error("the running turn's id is not known")
            const stopped = await this.dp.stopTurn(threadId, turnId)
            if (!stopped.running) {
              this.emit(threadId, {
                type: "notice",
                text: `stopped the turn: its events were unreachable for ${Math.round(giveUpMs / 1000)}s`,
              })
              break
            }
          } catch (stopError) {
            if (!warned) {
              warned = true
              this.emit(threadId, {
                type: "notice",
                text: `cannot reach the runner (${message(stopError)}); the turn may still be running, trying again`,
              })
            }
          }
        } else if (failures === 1) {
          this.emit(threadId, {
            type: "notice",
            text: `lost the turn's events (${message(error)}); trying again`,
          })
        }
        await new Promise(r => setTimeout(r, retryMs * 2 ** Math.min(failures - 1, 4)))
        continue
      }
      failures = 0
      warned = false
      // A restarted runner numbers its events from 1 again: start over from its first event.
      if (known.generation !== result.generation) {
        this.store.update(threadId, {
          generation: result.generation,
          ...(known.generation !== undefined ? { cursor: 0 } : {}),
        })
        if (known.generation !== undefined) continue
      }
      let ended = false
      let cursor = known.cursor ?? 0
      for (const event of result.events) {
        cursor = event.seq
        if (event.kind === "agent")
          this.emit(threadId, { type: "agent", event: event.event as Record<string, unknown> })
        else if (event.kind === "stderr")
          this.emit(threadId, { type: "stderr", text: String(event.text) })
        else if (event.kind === "turn-start")
          this.store.update(threadId, { turnId: String(event.turnId) })
        else if (event.kind === "turn-end") {
          ended = true
          if (event.error)
            this.emit(threadId, { type: "notice", text: `turn failed: ${event.error}` })
        }
      }
      this.store.update(threadId, {
        cursor,
        ...(result.sessionId ? { sessionId: result.sessionId } : {}),
      })
      if (ended || (!result.running && result.events.length === 0)) break
    }
    this.pumping.delete(threadId)
    if (pending) this.emit(threadId, { type: "approvals", pending: [] })
    await this.finish(threadId)
  }

  /** The turn is over: its token and the sandbox's credential rules go, then the thread is idle. */
  private async finish(threadId: string): Promise<void> {
    const sandboxId = this.store.get(threadId)?.sandboxId
    if (sandboxId) {
      try {
        await this.dp.endTurn(threadId, sandboxId)
      } catch (error) {
        this.emit(threadId, {
          type: "notice",
          text: `could not end the turn's access: ${message(error)}`,
        })
      }
    }
    this.setState(threadId, "idle")
    this.armIdle(threadId)
  }

  private clearIdle(threadId: string): void {
    clearTimeout(this.idleTimers.get(threadId))
    this.idleTimers.delete(threadId)
    // A pause that already fired and waits for the lock is cancelled too.
    this.idleGen.set(threadId, (this.idleGen.get(threadId) ?? 0) + 1)
  }

  private armIdle(threadId: string): void {
    this.clearIdle(threadId)
    const gen = this.idleGen.get(threadId)
    this.idleTimers.set(
      threadId,
      setTimeout(() => void this.pauseIdle(threadId, gen), this.options.idlePauseMs),
    )
  }

  private pauseIdle(threadId: string, gen: number | undefined): Promise<void> {
    // Under the thread's lock, and only if the thread is still idle when the lock is ours: a
    // message that arrived first has started a turn (or is waiting behind this pause, and then
    // resumes the sandbox).
    return this.locks.run(threadId, async () => {
      const thread = this.store.get(threadId)
      if (!thread?.sandboxId || thread.state !== "idle") return
      // Something happened in the thread since the timer was set (a message, a command).
      if (this.idleGen.get(threadId) !== gen || (this.execs.get(threadId) ?? 0) > 0) return
      try {
        const result = await this.dp.pause(threadId, thread.sandboxId)
        if (result.paused) {
          this.setState(threadId, "paused")
          this.emit(threadId, {
            type: "notice",
            text: `paused sandbox ${thread.sandboxId} after idling`,
          })
        } else {
          this.emit(threadId, { type: "notice", text: `not paused: ${result.reason}` })
        }
      } catch (error) {
        this.emit(threadId, { type: "notice", text: `pause failed: ${message(error)}` })
      }
    })
  }

  /**
   * Runs a shell command in the thread's sandbox, resuming it first if it is paused. It runs
   * under the thread's lock, so a user switch (which sweeps the sandbox) waits for it and sees
   * its processes, and the thread is not paused while it runs.
   */
  async exec(threadId: string, command: string): Promise<void> {
    this.execs.set(threadId, (this.execs.get(threadId) ?? 0) + 1)
    this.clearIdle(threadId)
    try {
      await this.locks.run(threadId, async () => {
        let thread = this.store.get(threadId)
        if (!thread) throw new Error(`no thread ${threadId}`)
        if (!thread.sandboxId || thread.state === "paused") {
          thread = await this.open(thread)
          if (thread.state === "paused") this.setState(threadId, "idle")
        }
        // The command runs as whoever owns the sandbox's processes now.
        if (!thread.owner) this.store.update(threadId, { owner: thread.user })
        const result = await this.dp.exec(thread.sandboxId as string, command)
        this.emit(threadId, { type: "exec", command, ...result })
      })
    } finally {
      const left = (this.execs.get(threadId) ?? 1) - 1
      if (left > 0) this.execs.set(threadId, left)
      else this.execs.delete(threadId)
      if (left === 0 && this.store.get(threadId)?.state !== "running") this.armIdle(threadId)
    }
  }

  /**
   * Passes an approver's decision to the connector, which decides whether it counts. The demo
   * has no sign-in, so `by` is the identity the UI claims; in your product your own sign-in
   * decides who is approving.
   */
  async decide(threadId: string, id: string, approve: boolean, by: string): Promise<Decided> {
    const result = await this.dp.decide({ id, threadId, approve, by })
    this.emit(threadId, {
      type: "notice",
      text: result.decided
        ? `${by} ${approve ? "approved" : "denied"} the write`
        : `${by}'s decision was not accepted: ${result.reason ?? "that write is no longer waiting"}`,
    })
    return result
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
