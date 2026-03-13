# SyncReconcileRequestCurrentReleaseLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseLifecycle } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseLifecycle = "live-on-setup";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```