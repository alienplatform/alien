# SyncAcquireResponseStackStateLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { SyncAcquireResponseStackStateLifecycle } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStackStateLifecycle = "live";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```