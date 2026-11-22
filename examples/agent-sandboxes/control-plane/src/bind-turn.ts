// Binds a sandbox to the user who is asking, before their turn starts.
//
// The connector issues a token for (thread, user); the runner installs it on the sandbox's
// connector link, where the egress gateway adds it to every request as
// `Authorization: Bearer <token>`. The call returns after the gateway has acknowledged the rule,
// so the turn's first request already carries the new user's token, and the previous user's
// token no longer works anywhere.

import type { DataPlane } from "./data-plane"

export interface Binding {
  bound: boolean
  reason?: string
}

export async function bindTurn(dp: DataPlane, sandbox: { threadId: string; sandboxId: string }, user: string): Promise<Binding> {
  const { token } = await dp.issueToken(sandbox.threadId, user)
  return dp.bindTurn(sandbox.sandboxId, token)
}
