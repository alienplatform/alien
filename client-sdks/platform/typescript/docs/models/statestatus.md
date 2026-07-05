# StateStatus

Deployment status in the deployment lifecycle.

For observe-only deployments with no release or stack state, `Running`
means the Operator is attached. Connectivity comes from `lastHeartbeatAt`;
resource health comes from inventory and resource heartbeat data.

## Example Usage

```typescript
import { StateStatus } from "@alienplatform/platform-api/models";

let value: StateStatus = "running";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```