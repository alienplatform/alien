# DeploymentStackStateStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { DeploymentStackStateStatus } from "@aliendotdev/platform-api/models";

let value: DeploymentStackStateStatus = "provisioning";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "deleted" | "refresh-failed"
```