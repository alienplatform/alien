# DataAzureManagedRedis

Azure Managed Redis.

## Example Usage

```typescript
import { DataAzureManagedRedis } from "@alienplatform/platform-api/models";

let value: DataAzureManagedRedis = {
  clusterName: "<value>",
  config: {},
  metrics: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azureManagedRedis",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `clusterName`                                                                                                                        | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `config`                                                                                                                             | [models.SyncReconcileRequestConfig3](../models/syncreconcilerequestconfig3.md)                                                       | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `metrics`                                                                                                                            | [models.SyncReconcileRequestMetrics3](../models/syncreconcilerequestmetrics3.md)                                                     | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [models.ResourceHeartbeatStatus39](../models/resourceheartbeatstatus39.md)                                                           | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"azureManagedRedis"*                                                                                                                | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |