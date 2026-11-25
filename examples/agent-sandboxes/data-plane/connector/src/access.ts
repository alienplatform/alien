// Who may do what through the connector. A real product reads this from its own user store;
// the demo has a few users so a switch mid-thread shows access changing, and a manager who
// approves writes.

export type Action = "orders:read" | "orders:write" | "orders:approve" | "scanner:read"

export const ACCESS: Record<string, readonly Action[]> = {
  alice: ["orders:read", "orders:write", "scanner:read"],
  bob: ["orders:read", "scanner:read"],
  manager: ["orders:read", "orders:approve"],
}

export function allowed(user: string, action: Action): boolean {
  return ACCESS[user]?.includes(action) ?? false
}
