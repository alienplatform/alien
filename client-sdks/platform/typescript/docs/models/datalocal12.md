# DataLocal12

Local: containers Docker still holds for this sandbox.

## Example Usage

```typescript
import { DataLocal12 } from "@alienplatform/platform-api/models";

let value: DataLocal12 = {
  activeSessions: 111146,
  routeServing: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `activeSessions`                                                                                                     | *number*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `routeServing`                                                                                                       | *boolean*                                                                                                            | :heavy_check_mark:                                                                                                   | Whether the loopback route is serving in this process. False after a manager restart until<br/>the next tick rebinds it. |
| `status`                                                                                                             | [models.SyncReconcileRequestStatus76](../models/syncreconcilerequeststatus76.md)                                     | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `backend`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |