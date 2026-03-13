# SyncReconcileRequestStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { SyncReconcileRequestStatus } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestStatus = "initial-setup";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```