# SyncReconcileRequestTargetReleaseLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { SyncReconcileRequestTargetReleaseLifecycle } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestTargetReleaseLifecycle = "live-on-setup";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```