# ManagerDeploymentStatus

Deployment status of the internal deployment

## Example Usage

```typescript
import { ManagerDeploymentStatus } from "@alienplatform/platform-api/models";

let value: ManagerDeploymentStatus = "teardown-failed";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "waiting-for-machines" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```