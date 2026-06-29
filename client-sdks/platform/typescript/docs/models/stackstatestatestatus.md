# StackStateStateStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { StackStateStateStatus } from "@alienplatform/platform-api/models";

let value: StackStateStateStatus = "deleted";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "teardown-required" | "deleted" | "refresh-failed"
```