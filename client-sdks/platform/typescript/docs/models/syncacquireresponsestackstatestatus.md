# SyncAcquireResponseStackStateStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { SyncAcquireResponseStackStateStatus } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStackStateStatus = "delete-failed";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "deleted" | "refresh-failed"
```