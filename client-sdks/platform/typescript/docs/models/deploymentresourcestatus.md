# DeploymentResourceStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { DeploymentResourceStatus } from "@alienplatform/platform-api/models";

let value: DeploymentResourceStatus = "update-failed";

// Open enum: unrecognized values are captured as Unrecognized<string>
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "teardown-required" | "deleted" | "refresh-failed" | Unrecognized<string>
```