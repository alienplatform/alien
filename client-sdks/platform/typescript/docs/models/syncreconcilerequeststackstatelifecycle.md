# SyncReconcileRequestStackStateLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { SyncReconcileRequestStackStateLifecycle } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStackStateLifecycle = "live-on-setup";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```