// Per-turn tokens. The control plane asks for one before each turn and installs it on the
// sandbox's link (the gateway adds it to requests to this connector); the sandbox itself only
// ever holds a placeholder.
//
// Tokens live in this process's memory (the connector runs one replica). A turn's token lives
// until the turn ends (`revoke`), and a new token for a thread replaces the thread's previous
// one, so a user switch ends the old user's access here as well as at the gateway. The time
// limit only matters when nobody revokes it: it must outlast the longest turn.

import { randomBytes } from "node:crypto"

export interface TurnGrant {
  token: string
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
    this.byToken.set(token, { token, threadId, user, expiresAt })
    this.byThread.set(threadId, token)
    return { token, expiresAt }
  }

  /** Ends the thread's token, if it has one. */
  revoke(threadId: string): boolean {
    const token = this.byThread.get(threadId)
    if (!token) return false
    this.byToken.delete(token)
    this.byThread.delete(threadId)
    return true
  }

  /** Whether this exact token is still the thread's live token (not revoked, replaced or expired). */
  isCurrent(token: string): boolean {
    const grant = this.byToken.get(token)
    return grant !== undefined && grant.expiresAt > this.now() && this.byThread.get(grant.threadId) === token
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
