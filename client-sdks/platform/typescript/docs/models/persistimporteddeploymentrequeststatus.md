# PersistImportedDeploymentRequestStatus

Deployment status in the deployment lifecycle

## Example Usage

```typescript
import { PersistImportedDeploymentRequestStatus } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestStatus = "deleting";
```

## Values

```typescript
"pending" | "preflights-failed" | "initial-setup" | "initial-setup-failed" | "provisioning" | "provisioning-failed" | "running" | "refresh-failed" | "update-pending" | "updating" | "update-failed" | "delete-pending" | "deleting" | "delete-failed" | "teardown-required" | "teardown-failed" | "deleted" | "error"
```