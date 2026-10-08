# VolumeRestoreStatus

pending: sent to the deployment until the container reports it done. If the restore fails, the container's resource shows the error and the request stays pending: retry the deployment or cancel the request. completed: the container reported the restored volume. cancelled: withdrawn before it completed. A cancelled request becomes completed if its volume was already swapped when the cancellation arrived, since the container then finishes the restore.

## Example Usage

```typescript
import { VolumeRestoreStatus } from "@alienplatform/platform-api/models";

let value: VolumeRestoreStatus = "cancelled";
```

## Values

```typescript
"pending" | "completed" | "cancelled"
```
