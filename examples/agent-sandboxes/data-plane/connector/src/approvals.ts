// Writes the connector holds until someone approves them in the control plane. The HTTP request
// from the sandbox stays open while it waits, so the agent sees the outcome as its command's
// output. Held in memory, like the tokens.

import { randomUUID } from "node:crypto"

export interface PendingWrite {
  id: string
  threadId: string
  user: string
  method: string
  path: string
  body: unknown
  requestedAt: string
}

export type Decision =
  | { approved: true; by: string }
  | { approved: false; by: string; reason: string }

export class Approvals {
  private pending = new Map<string, { write: PendingWrite; token: string; settle: (d: Decision) => void }>()

  constructor(private readonly timeoutMs: number) {}

  /**
   * Holds a write until `decide` or the timeout, whichever comes first. `token` is the turn
   * token the request came with; it stays here, never in the list the control plane reads.
   */
  hold(write: Omit<PendingWrite, "id" | "requestedAt">, token: string): { write: PendingWrite; decision: Promise<Decision> } {
    const full: PendingWrite = { ...write, id: randomUUID(), requestedAt: new Date().toISOString() }
    const decision = new Promise<Decision>(resolve => {
      const timer = setTimeout(() => {
        this.pending.delete(full.id)
        resolve({ approved: false, by: "timeout", reason: "nobody approved the write in time" })
      }, this.timeoutMs)
      this.pending.set(full.id, {
        write: full,
        token,
        settle: d => {
          clearTimeout(timer)
          this.pending.delete(full.id)
          resolve(d)
        },
      })
    })
    return { write: full, decision }
  }

  list(threadId?: string): PendingWrite[] {
    return [...this.pending.values()]
      .map(p => p.write)
      .filter(w => threadId === undefined || w.threadId === threadId)
  }

  /** The held write, with the token that asked for it. */
  get(id: string): { write: PendingWrite; token: string } | undefined {
    const entry = this.pending.get(id)
    return entry && { write: entry.write, token: entry.token }
  }

  /** False when the write is no longer pending (decided, or timed out). */
  decide(id: string, decision: Decision): boolean {
    const entry = this.pending.get(id)
    if (!entry) return false
    entry.settle(decision)
    return true
  }
}
