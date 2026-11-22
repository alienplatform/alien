// Per-turn tokens. The control plane asks for one before each turn and installs it on the
// sandbox's link (the gateway adds it to requests to this connector); the sandbox itself only
// ever holds a placeholder.
//
// Tokens live in this process's memory (the connector runs one replica). A new token for a
// thread replaces the thread's previous one, so a user switch ends the old user's access here
// as well as at the gateway.

import { randomBytes } from "node:crypto"

export interface TurnGrant {
  threadId: string
  user: string
  expiresAt: number
}

export class TurnTokens {
  private byToken = new Map<string, TurnGrant>()
  private byThread = new Map<string, string>()

  constructor(
    private readonly ttlMs: number,
    private readonly now: () => number = Date.now,
  ) {}

  issue(threadId: string, user: string): { token: string; expiresAt: number } {
    const previous = this.byThread.get(threadId)
    if (previous) this.byToken.delete(previous)
    const token = randomBytes(32).toString("base64url")
    const expiresAt = this.now() + this.ttlMs
    this.byToken.set(token, { threadId, user, expiresAt })
    this.byThread.set(threadId, token)
    return { token, expiresAt }
  }

  /** The grant for an `Authorization: Bearer <token>` header, or why there is none. */
  check(header: string | undefined): TurnGrant | { error: string } {
    const token = header?.startsWith("Bearer ") ? header.slice(7) : undefined
    if (!token || token === "placeholder") return { error: "no turn credentials on this request" }
    const grant = this.byToken.get(token)
    if (!grant) return { error: "unknown or replaced turn token" }
    if (grant.expiresAt <= this.now()) {
      this.byToken.delete(token)
      return { error: "turn token expired" }
    }
    return grant
  }
}
