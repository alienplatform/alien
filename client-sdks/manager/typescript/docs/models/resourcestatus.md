# ResourceStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { ResourceStatus } from "@alienplatform/manager-api/models";

let value: ResourceStatus = "deleting";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "deleted" | "refresh-failed"
```