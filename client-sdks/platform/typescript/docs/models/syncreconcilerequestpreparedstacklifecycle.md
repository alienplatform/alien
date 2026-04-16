# SyncReconcileRequestPreparedStackLifecycle

Describes the lifecycle of a resource within a stack, determining how it's managed and deployed.

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackLifecycle } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackLifecycle = "frozen";
```

## Values

```typescript
"frozen" | "live" | "live-on-setup"
```