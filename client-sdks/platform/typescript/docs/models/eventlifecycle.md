# EventLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { EventLifecycle } from "@aliendotdev/platform-api/models";

let value: EventLifecycle = "frozen";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```