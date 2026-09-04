# DenyAccessRequestMaxRisk

How risky an operation is (declared by the plugin metadata).

## Example Usage

```typescript
import { DenyAccessRequestMaxRisk } from "@alienplatform/platform-api/models/operations";

let value: DenyAccessRequestMaxRisk = "read-only";
```

## Values

```typescript
"read-only" | "mutating" | "destructive"
```