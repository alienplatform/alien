# SetupProgress

For a pending deployment: whether its external setup is awaited, started or rolled back

## Example Usage

```typescript
import { SetupProgress } from "@alienplatform/platform-api/models";

let value: SetupProgress = "awaiting";
```

## Values

```typescript
"awaiting" | "started" | "rolled-back"
```