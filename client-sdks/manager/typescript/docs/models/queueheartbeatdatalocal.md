# QueueHeartbeatDataLocal

## Example Usage

```typescript
import { QueueHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: QueueHeartbeatDataLocal = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `path`                                                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `serviceStatus`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.QueueHeartbeatStatus](../models/queueheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"local"*                                                        | :heavy_check_mark:                                               | N/A                                                              |