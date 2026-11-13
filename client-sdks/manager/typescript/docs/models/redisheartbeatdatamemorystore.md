# RedisHeartbeatDataMemorystore

GCP Memorystore for Valkey.

## Example Usage

```typescript
import { RedisHeartbeatDataMemorystore } from "@alienplatform/manager-api/models";

let value: RedisHeartbeatDataMemorystore = {
  config: {},
  instanceName: "<value>",
  metrics: {},
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
  backend: "memorystore",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `config`                                                                                                                             | [models.RedisObservedConfig](../models/redisobservedconfig.md)                                                                       | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `instanceName`                                                                                                                       | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `metrics`                                                                                                                            | [models.RedisMetricsSample](../models/redismetricssample.md)                                                                         | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [models.RedisHeartbeatStatus](../models/redisheartbeatstatus.md)                                                                     | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"memorystore"*                                                                                                                      | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
