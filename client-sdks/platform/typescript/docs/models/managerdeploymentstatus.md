# ManagerDeploymentStatus

Deployment status of the internal deployment

## Example Usage

```typescript
import { ManagerDeploymentStatus } from "@alienplatform/platform-api/models";

let value: ManagerDeploymentStatus = "delete-failed";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```