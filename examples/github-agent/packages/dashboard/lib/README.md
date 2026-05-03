# Library Files

## Server-Side Only

These files contain server-side code and should **NOT** be imported in client components:

- `config.ts` - Contains `alien` SDK client (uses Node.js environment)
- `arc.ts` - Commands client with dynamic Node.js imports (`node:fs/promises`)
- `deployment-groups.ts` - Uses database and SDK (server-side only)
- `db.ts` - Database connection (server-side only)
- `schema.ts` - Database schema (server-side only)
- `auth.ts` - Server-side auth configuration

## Client-Side Safe

These files can be imported in client components:

- `queries.ts` - React Query hooks (uses fetch, not direct SDK)
- `query-client.tsx` - React Query provider
- `auth-client.ts` - Client-side auth hooks
- `utils.ts` - Utility functions (pure functions)

## Important Rules

1. **Never import `config.ts`, `arc.ts`, or `db.ts` in client components** - This will cause webpack errors due to Node.js-specific imports.

2. **Use API routes for server-side operations** - Client components should call API routes (e.g., `/api/agents`) which then use the SDK server-side.

3. **React Query hooks in `queries.ts` use `fetch`** - They don't directly use the SDK, making them safe for client components.

## Examples

### ❌ Bad (Client Component)

```typescript
"use client"
import { alien } from "@/lib/config" // ERROR: Node.js code in browser

export default function MyComponent() {
  // This will fail at build time
}
```

### ✅ Good (Client Component)

```typescript
"use client"
import { useAgents } from "@/lib/queries" // OK: Uses fetch internally

export default function MyComponent() {
  const { data: agents } = useAgents()
  // Works perfectly!
}
```

### ✅ Good (Server Component)

```typescript
import { alien, config } from "@/lib/config" // OK: Server-side

export default async function MyPage() {
  const agents = await alien.deployments.list({
    workspace: config.workspace,
    deploymentGroup: "dg_xxx"
  })
  // Works perfectly!
}
```

### ✅ Good (API Route)

```typescript
import { alien, config } from "@/lib/config" // OK: Server-side

export async function GET() {
  const agents = await alien.deployments.list({
    workspace: config.workspace,
    deploymentGroup: "dg_xxx"
  })
  return Response.json({ agents })
}
```

