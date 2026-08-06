# SyncReconcileResponseSslMode

TLS policy for an operator-provided / BYO Postgres database.

Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the
connection settings exposed by every supported SDK.

## Example Usage

```typescript
import { SyncReconcileResponseSslMode } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseSslMode = "verify-full";
```

## Values

```typescript
"verify-full" | "disable"
```