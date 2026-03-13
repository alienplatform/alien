# SyncAcquireResponseStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { SyncAcquireResponseStatus } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseStatus = "running";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```