// Per-thread event buffers. A turn's output is buffered here so the control plane can read it
// with a long poll over the command channel, which is request/response.

export type RunnerEvent =
  | { seq: number; kind: "turn-start"; turnId: string; user: string }
  | { seq: number; kind: "agent"; event: Record<string, unknown> }
  | { seq: number; kind: "stderr"; text: string }
  | { seq: number; kind: "turn-end"; turnId: string; exitCode: number | null; error?: string }

export type NewEvent = RunnerEvent extends infer E ? (E extends RunnerEvent ? Omit<E, "seq"> : never) : never

const MAX_EVENTS = 2000

export class ThreadEvents {
  running = false
  sessionId: string | undefined
  private events: RunnerEvent[] = []
  private seq = 0
  private waiters: Array<() => void> = []

  push(event: NewEvent): void {
    this.seq += 1
    this.events.push({ ...event, seq: this.seq } as RunnerEvent)
    if (this.events.length > MAX_EVENTS) this.events.splice(0, this.events.length - MAX_EVENTS)
    for (const wake of this.waiters.splice(0)) wake()
  }

  /** Events after `after`; waits up to `waitMs` for the first one when there are none yet. */
  async after(after: number, waitMs: number): Promise<RunnerEvent[]> {
    const ready = () => this.events.filter(e => e.seq > after)
    if (ready().length > 0 || waitMs <= 0) return ready()
    await new Promise<void>(resolve => {
      const timer = setTimeout(resolve, waitMs)
      this.waiters.push(() => {
        clearTimeout(timer)
        resolve()
      })
    })
    return ready()
  }
}

/** Splits a byte stream into lines; the last partial line is kept until it completes. */
export class LineSplitter {
  private rest = ""

  feed(chunk: string): string[] {
    const text = this.rest + chunk
    const lines = text.split("\n")
    this.rest = lines.pop() ?? ""
    return lines.filter(line => line.trim() !== "")
  }

  flush(): string[] {
    const last = this.rest.trim()
    this.rest = ""
    return last ? [last] : []
  }
}
