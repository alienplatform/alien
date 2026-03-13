# DeploymentStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { DeploymentStatus } from "@alienplatform/platform-api/models";

let value: DeploymentStatus = "update-failed";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```