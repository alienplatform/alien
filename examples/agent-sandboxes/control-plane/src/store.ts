// Threads, in a JSON file. What must survive a control-plane restart: which sandbox and which
// Claude Code session belong to a thread, and who is asking.

import { existsSync, readFileSync, renameSync, writeFileSync } from "node:fs"

export interface Thread {
  id: string
  title: string
  user: string
  sandboxId?: string
  sessionId?: string
  state: "new" | "running" | "idle" | "paused"
  lastActivity: number
}

export class ThreadStore {
  private threads: Record<string, Thread>

  constructor(private readonly path?: string) {
    this.threads = path && existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {}
  }

  list(): Thread[] {
    return Object.values(this.threads).sort((a, b) => b.lastActivity - a.lastActivity)
  }

  get(id: string): Thread | undefined {
    return this.threads[id]
  }

  put(thread: Thread): Thread {
    this.threads[thread.id] = thread
    if (this.path) {
      writeFileSync(`${this.path}.tmp`, JSON.stringify(this.threads, null, 2))
      renameSync(`${this.path}.tmp`, this.path)
    }
    return thread
  }

  update(id: string, patch: Partial<Thread>): Thread {
    const thread = this.get(id)
    if (!thread) throw new Error(`no thread ${id}`)
    return this.put({ ...thread, ...patch })
  }
}
