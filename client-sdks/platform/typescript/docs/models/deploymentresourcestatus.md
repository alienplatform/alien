# DeploymentResourceStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { DeploymentResourceStatus } from "@alienplatform/platform-api/models";

let value: DeploymentResourceStatus = "update-failed";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "teardown-required" | "deleted" | "refresh-failed"
```