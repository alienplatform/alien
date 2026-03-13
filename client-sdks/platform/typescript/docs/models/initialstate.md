# InitialState

Initial state (PENDING_UPLOAD if params require upload, PENDING if inline)

## Example Usage

```typescript
import { InitialState } from "@aliendotdev/platform-api/models";

let value: InitialState = "DISPATCHED";
```

## Values

```typescript
"PENDING_UPLOAD" | "PENDING" | "DISPATCHED" | "SUCCEEDED" | "FAILED" | "EXPIRED"
```