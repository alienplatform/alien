# EventStatus

Represents the high-level status of a resource during its lifecycle.

## Example Usage

```typescript
import { EventStatus } from "@aliendotdev/platform-api/models";

let value: EventStatus = "deleting";
```

## Values

```typescript
"pending" | "provisioning" | "provision-failed" | "running" | "updating" | "update-failed" | "deleting" | "delete-failed" | "deleted" | "refresh-failed"
```