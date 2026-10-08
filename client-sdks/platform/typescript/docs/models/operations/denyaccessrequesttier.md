# DenyAccessRequestTier

How risky an operation is (declared by the plugin metadata).

## Example Usage

```typescript
import { DenyAccessRequestTier } from "@alienplatform/platform-api/models/operations";

let value: DenyAccessRequestTier = "mutating";
```

## Values

```typescript
"read-only" | "mutating" | "destructive"
```