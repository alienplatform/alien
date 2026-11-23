// Who may do what through the connector. A real product reads this from its own user store;
// the demo has two users so a switch mid-thread shows access changing.

export type Action = "orders:read" | "orders:write"

export const ACCESS: Record<string, readonly Action[]> = {
  alice: ["orders:read", "orders:write"],
  bob: ["orders:read"],
}

export function allowed(user: string, action: Action): boolean {
  return ACCESS[user]?.includes(action) ?? false
}
