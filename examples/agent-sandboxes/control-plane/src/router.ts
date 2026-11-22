// Thread router: one sandbox per thread.
//
// A message opens the thread's sandbox (creating it, or resuming it if it was paused), binds it
// to the user who is asking, and only then starts the turn. While the turn runs, the router
// long-polls the runner for the agent's events and the connector for writes waiting for
// approval, and fans both out to the browser. A thread left idle is paused.

import { bindTurn } from "./bind-turn"
import type { DataPlane, PendingWrite } from "./data-plane"
import type { Thread, ThreadStore } from "./store"

export type UiEvent =
  | { type: "user"; user: string; text: string }
  | { type: "agent"; event: Record<string, unknown> }
  | { type: "stderr"; text: string }
  | { type: "notice"; text: string }
  | { type: "state"; state: Thread["state"] }
  | { type: "approvals"; pending: PendingWrite[] }
  | { type: "exec"; command: string; exitCode: number | null; stdout: string; stderr: string }

type Listener = (event: UiEvent) => void

export class ThreadRouter {
  private logs = new Map<string, UiEvent[]>()
  private listeners = new Map<string, Set<Listener>>()
  private cursors = new Map<string, number>()
  private idleTimers = new Map<string, ReturnType<typeof setTimeout>>()

  constructor(
    private readonly dp: DataPlane,
    private readonly store: ThreadStore,
    private readonly options: { idlePauseMs: number; pollMs?: number },
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
    log.push(event)
    this.logs.set(threadId, log)
    for (const listener of this.listeners.get(threadId) ?? []) listener(event)
  }

  private setState(threadId: string, state: Thread["state"]): Thread {
    const thread = this.store.update(threadId, { state, lastActivity: Date.now() })
    this.emit(threadId, { type: "state", state })
    return thread
  }

  /** Opens the thread's sandbox, resuming it if it was paused. */
  private async open(thread: Thread): Promise<Thread> {
    const opened = await this.dp.open(thread.id, thread.sandboxId)
    if (opened.resumed) this.emit(thread.id, { type: "notice", text: `resumed sandbox ${opened.sandboxId}` })
    if (opened.created) {
      this.emit(thread.id, { type: "notice", text: `created sandbox ${opened.sandboxId}` })
      return this.store.update(thread.id, { sandboxId: opened.sandboxId, sessionId: undefined })
    }
    return this.store.update(thread.id, { sandboxId: opened.sandboxId })
  }

  /** Starts a turn. Resolves once the turn has started; its events arrive through `subscribe`. */
  async send(threadId: string, user: string, text: string): Promise<void> {
    let thread = this.store.get(threadId)
    if (!thread) throw new Error(`no thread ${threadId}`)
    if (thread.state === "running") throw new Error("this thread is already answering")
    this.clearIdle(threadId)
    if (thread.user !== user) this.emit(threadId, { type: "notice", text: `user switched to ${user}` })
    this.store.update(threadId, { user })
    this.emit(threadId, { type: "user", user, text })
    this.setState(threadId, "running")
    try {
      thread = await this.open(thread)
      const sandboxId = thread.sandboxId as string
      // The turn does not start until the gateway has the new user's token.
      const binding = await bindTurn(this.dp, { threadId, sandboxId }, user)
      this.emit(threadId, {
        type: "notice",
        text: binding.bound ? `sandbox bound to ${user}` : `sandbox not bound: ${binding.reason}`,
      })
      await this.dp.turn({ threadId, sandboxId, sessionId: thread.sessionId, user, text })
    } catch (error) {
      this.emit(threadId, { type: "notice", text: `turn failed to start: ${message(error)}` })
      this.finish(threadId)
      throw error
    }
    void this.pump(threadId)
  }

  private async pump(threadId: string): Promise<void> {
    const pollMs = this.options.pollMs ?? 2000
    let pending = ""
    try {
      for (;;) {
        const after = this.cursors.get(threadId) ?? 0
        const [result, writes] = await Promise.all([
          this.dp.events(threadId, after, pollMs),
          this.dp.approvals(threadId),
        ])
        const key = writes.map(w => w.id).join(",")
        if (key !== pending) {
          pending = key
          this.emit(threadId, { type: "approvals", pending: writes })
        }
        let ended = false
        for (const event of result.events) {
          this.cursors.set(threadId, event.seq)
          if (event.kind === "agent") this.emit(threadId, { type: "agent", event: event.event as Record<string, unknown> })
          else if (event.kind === "stderr") this.emit(threadId, { type: "stderr", text: String(event.text) })
          else if (event.kind === "turn-end") {
            ended = true
            if (event.error) this.emit(threadId, { type: "notice", text: `turn failed: ${event.error}` })
          }
        }
        if (result.sessionId) this.store.update(threadId, { sessionId: result.sessionId })
        if (ended || (!result.running && result.events.length === 0)) break
      }
    } catch (error) {
      this.emit(threadId, { type: "notice", text: `lost the turn's events: ${message(error)}` })
    }
    if (pending) this.emit(threadId, { type: "approvals", pending: [] })
    this.finish(threadId)
  }

  private finish(threadId: string): void {
    this.setState(threadId, "idle")
    this.armIdle(threadId)
  }

  private clearIdle(threadId: string): void {
    clearTimeout(this.idleTimers.get(threadId))
    this.idleTimers.delete(threadId)
  }

  private armIdle(threadId: string): void {
    this.clearIdle(threadId)
    this.idleTimers.set(
      threadId,
      setTimeout(() => void this.pauseIdle(threadId), this.options.idlePauseMs),
    )
  }

  private async pauseIdle(threadId: string): Promise<void> {
    const thread = this.store.get(threadId)
    if (!thread?.sandboxId || thread.state !== "idle") return
    try {
      const result = await this.dp.pause(threadId, thread.sandboxId)
      if (result.paused) {
        this.setState(threadId, "paused")
        this.emit(threadId, { type: "notice", text: `paused sandbox ${thread.sandboxId} after idling` })
      } else {
        this.emit(threadId, { type: "notice", text: `not paused: ${result.reason}` })
      }
    } catch (error) {
      this.emit(threadId, { type: "notice", text: `pause failed: ${message(error)}` })
    }
  }

  /** Runs a shell command in the thread's sandbox, resuming it first if it is paused. */
  async exec(threadId: string, command: string): Promise<void> {
    let thread = this.store.get(threadId)
    if (!thread) throw new Error(`no thread ${threadId}`)
    if (!thread.sandboxId || thread.state === "paused") {
      thread = await this.open(thread)
      if (thread.state === "paused") this.setState(threadId, "idle")
    }
    const result = await this.dp.exec(thread.sandboxId as string, command)
    this.emit(threadId, { type: "exec", command, ...result })
    if (this.store.get(threadId)?.state !== "running") this.armIdle(threadId)
  }

  async decide(threadId: string, id: string, approve: boolean, by: string): Promise<boolean> {
    const decided = await this.dp.decide(id, approve, by)
    this.emit(threadId, {
      type: "notice",
      text: decided ? `${by} ${approve ? "approved" : "denied"} the write` : "that write is no longer waiting",
    })
    return decided
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
