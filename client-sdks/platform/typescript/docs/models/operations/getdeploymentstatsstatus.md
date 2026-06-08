# GetDeploymentStatsStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { GetDeploymentStatsStatus } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentStatsStatus = "updating";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```