# ExternalPostgresSslMode

TLS policy for an operator-provided / BYO Postgres database.

Unlike libpq's ambiguous `prefer` mode, both choices map exactly to the
connection settings exposed by every supported SDK.

## Example Usage

```typescript
import { ExternalPostgresSslMode } from "@alienplatform/manager-api/models";

let value: ExternalPostgresSslMode = "verify-full";
```

## Values

```typescript
"verify-full" | "disable"
```