// Binds a sandbox to the user who is asking, before their turn starts.
//
// The connector issues a token for (thread, user); the runner installs it on the sandbox's
// connector link, where the egress gateway adds it to every request as
// `Authorization: Bearer <token>`. The call returns after the gateway has acknowledged the rule,
// so the turn's first request already carries the new user's token, and the previous user's
// token no longer works anywhere. The token passes through the control plane on its way (the
// connector issues it over a command, the runner receives it over another); it is never logged
// or stored. A bind that does not succeed is an error: the turn must not start unbound.

import type { DataPlane } from "./data-plane"

export async function bindTurn(dp: DataPlane, sandbox: { threadId: string; sandboxId: string }, user: string): Promise<void> {
  const { token } = await dp.issueToken(sandbox.threadId, user)
  const binding = await dp.bindTurn(sandbox.sandboxId, token)
  if (!binding.bound) throw new Error(`the sandbox was not bound to ${user}: ${binding.reason ?? "no reason given"}`)
}
