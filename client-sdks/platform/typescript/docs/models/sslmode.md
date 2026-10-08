# SslMode

TLS policy for an operator-provided / BYO Postgres database.

Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the
connection settings exposed by every supported SDK.

## Example Usage

```typescript
import { SslMode } from "@alienplatform/platform-api/models";

let value: SslMode = "disable";
```

## Values

```typescript
"verify-full" | "disable"
```