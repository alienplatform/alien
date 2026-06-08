# DeploymentDetailResponseStackStateStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { DeploymentDetailResponseStackStateStatus } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseStackStateStatus = "deleting";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "teardown-required" | "deleted" | "refresh-failed"
```