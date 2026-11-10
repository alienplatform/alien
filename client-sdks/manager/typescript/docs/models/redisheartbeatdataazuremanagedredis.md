# RedisHeartbeatDataAzureManagedRedis

Azure Managed Redis.

## Example Usage

```typescript
import { RedisHeartbeatDataAzureManagedRedis } from "@alienplatform/manager-api/models";

let value: RedisHeartbeatDataAzureManagedRedis = {
  clusterName: "<value>",
  config: {},
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
  backend: "azureManagedRedis",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `clusterName`                                                                                                                        | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `config`                                                                                                                             | [models.RedisObservedConfig](../models/redisobservedconfig.md)                                                                       | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `metrics`                                                                                                                            | [models.RedisMetricsSample](../models/redismetricssample.md)                                                                         | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [models.RedisHeartbeatStatus](../models/redisheartbeatstatus.md)                                                                     | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"azureManagedRedis"*                                                                                                                | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
