# RedisHeartbeatDataLocal

Local Valkey container.

## Example Usage

```typescript
import { RedisHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: RedisHeartbeatDataLocal = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `port`                                                           | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `processRunning`                                                 | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.RedisHeartbeatStatus](../models/redisheartbeatstatus.md) | :heavy_check_mark:                                               | Same fields as every other resource heartbeat status.            |
| `version`                                                        | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"local"*                                                        | :heavy_check_mark:                                               | N/A                                                              |
