# DataLocal13

Local: containers Docker still holds for this sandbox.

## Example Usage

```typescript
import { DataLocal13 } from "@alienplatform/platform-api/models";

let value: DataLocal13 = {
  activeSessions: 879062,
  routeServing: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopping",
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
| `status`                                                                                                             | [models.ResourceHeartbeatStatus81](../models/resourceheartbeatstatus81.md)                                           | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `backend`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |