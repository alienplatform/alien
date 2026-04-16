# TypeScript Guidelines

## Quick Reference

- **Never use `any`** — only with human approval when absolutely necessary
- **Use `ts-pattern`** for type-safe pattern matching
- **Use AlienError** for all error handling:
  - `error.withContext(...)` when propagating AlienError instances
  - `AlienError.from(error).withContext(...)` for third-party errors
  - `new AlienError(ErrorDefinition.create(...))` for new errors
- **Use `defineError()`** with Zod schemas to define error types

---

## Error Handling with AlienError

### Core Rules

- **Always add context with `.withContext(ErrorDefinition.create({ ... }))` when propagating errors.**
- **For AlienError instances, call `.withContext()` directly:** `error.withContext(...)`
- **For third-party errors (JavaScript errors, libraries), wrap first:** `AlienError.from(error).withContext(...)`
- **Create new errors with `new AlienError(ErrorDefinition.create({ ... }))` when there's no source error.**
- **Don't repeat what the error template already says.**

### Examples

```typescript
// Adding context when propagating
try {
  return await database.insert("users", userData)
} catch (error) {
  throw (await AlienError.from(error)).withContext(
    UserCreationFailedError.create({
      email: userData.email,
      operation: "database_insert"
    })
  )
}

// Creating new errors without sources
if (!user.permissions.includes("admin")) {
  throw new AlienError(InsufficientPermissionsError.create({
    userId: user.id,
    requiredPermission: "admin",
    resource: "user_management"
  }))
}
```

---

## Designing Error Definitions

Use `defineError()` from `@alienplatform/core` with Zod schemas:

```typescript
import { defineError } from "@alienplatform/core"
import { z } from "zod/v4"

export const PaymentProviderUnreachableError = defineError({
  code: "PAYMENT_PROVIDER_UNREACHABLE",
  context: z.object({
    provider: z.string(),
    endpoint: z.string(),
    timeout: z.number(),
  }),
  message: ({ provider, endpoint, timeout }) => 
    `Payment provider '${provider}' unreachable at '${endpoint}' (timeout: ${timeout}ms)`,
  retryable: true,
  internal: false,
  httpStatusCode: 503,
})
```

- `retryable: true` for transient errors, `false` for permanent ones
- `internal: true` for sensitive errors that should be sanitized externally
