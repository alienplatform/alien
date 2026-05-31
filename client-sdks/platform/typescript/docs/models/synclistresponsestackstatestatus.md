# SyncListResponseStackStateStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { SyncListResponseStackStateStatus } from "@alienplatform/platform-api/models";

let value: SyncListResponseStackStateStatus = "provision-failed";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "deleted" | "refresh-failed"
```