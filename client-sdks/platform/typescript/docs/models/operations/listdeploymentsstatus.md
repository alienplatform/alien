# ListDeploymentsStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { ListDeploymentsStatus } from "@aliendotdev/platform-api/models/operations";

let value: ListDeploymentsStatus = "delete-pending";
```

## Values

```typescript
"pending" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "deleted"
```