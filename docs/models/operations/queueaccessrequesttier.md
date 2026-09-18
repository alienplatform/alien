# QueueAccessRequestTier

How risky an operation is (declared by the plugin metadata).

## Example Usage

```typescript
import { QueueAccessRequestTier } from "@alienplatform/platform-api/models/operations";

let value: QueueAccessRequestTier = "destructive";
```

## Values

```typescript
"read-only" | "mutating" | "destructive"
```