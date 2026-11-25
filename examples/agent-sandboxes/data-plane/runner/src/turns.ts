// Per-thread event buffers. A turn's output is buffered here so the control plane can read it
// with a long poll over the command channel, which is request/response.

export type RunnerEvent =
  | { seq: number; kind: "turn-start"; turnId: string; user: string }
  | { seq: number; kind: "agent"; event: Record<string, unknown> }
  | { seq: number; kind: "stderr"; text: string }
  | { seq: number; kind: "turn-end"; turnId: string; exitCode: number | null; error?: string }

export type NewEvent = RunnerEvent extends infer E
  ? E extends RunnerEvent
    ? Omit<E, "seq">
    : never
  : never

const MAX_EVENTS = 2000

/** Runs one function at a time, in the order they were asked for. */
export class Mutex {
  private tail: Promise<unknown> = Promise.resolve()

  run<T>(fn: () => Promise<T> | T): Promise<T> {
    const result = this.tail.then(fn)
    this.tail = result.catch(() => {})
    return result
  }
}

export class ThreadEvents {
  /** Serializes everything that changes the thread's sandbox: open, resume, turn start, pause. */
  readonly lock = new Mutex()
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

/** Starts a turn unless one is running. `start` runs inside the thread's lock, so a pause cannot interleave. */
export function startTurn(t: ThreadEvents, start: () => void): Promise<void> {
  return t.lock.run(() => {
    if (t.running) throw new Error("a turn is already running in this thread")
    t.running = true
    start()
  })
}

/**
 * Pauses the thread's sandbox only if no turn is running, checked inside the thread's lock: a
 * turn that arrives meanwhile waits for the pause to finish and then resumes it.
 */
export function pauseWhenIdle(
  t: ThreadEvents,
  canPause: () => Promise<boolean>,
  pause: () => Promise<void>,
): Promise<{ paused: boolean; reason?: string }> {
  return t.lock.run(async () => {
    if (t.running) throw new Error("a turn is running; pause after it ends")
    if (!(await canPause())) {
      return {
        paused: false,
        reason: "this platform keeps sandboxes running instead of pausing them",
      }
    }
    await pause()
    return { paused: true }
  })
}
