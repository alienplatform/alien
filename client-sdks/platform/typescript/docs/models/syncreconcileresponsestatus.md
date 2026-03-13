# SyncReconcileResponseStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { SyncReconcileResponseStatus } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseStatus = "deleted";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```