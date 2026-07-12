# SyncAcquireRequestStatus

Deployment status in the deployment lifecycle.

For observe-only deployments with no release or stack state, `Running`
means the Operator is attached. Connectivity comes from `lastHeartbeatAt`;
resource health comes from inventory and resource heartbeat data.

## Example Usage

```typescript
import { SyncAcquireRequestStatus } from "@alienplatform/platform-api/models";

let value: SyncAcquireRequestStatus = "delete-pending";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "waiting-for-machines" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```